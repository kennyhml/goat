//! Typed, heterogeneous batch operations.
//!
//! SAPs generic batch resource executes `application/http` subrequests in
//! insertion order and returns one multipart response part for each request.
//! [`BatchOperation`] preserves that order while [`BatchKey`] retains each
//! operations concrete response type.

use std::{any::Any, marker::PhantomData, sync::Arc};

use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use thiserror::Error;
use uuid::Uuid;

use super::{Operation, OperationKind};
use crate::{
    AdtRequest, AdtResponse, AdtUri, CategoryId, Client, CompatibilityError,
    NegotiableMediaVersion, OperationError, OperationResponse, Ready, ResponseError,
};

const BATCH_CATEGORY: CategoryId = CategoryId {
    scheme: "http://www.sap.com/adt/categories/system/communication/services",
    term: "batch",
};
const BATCH_MEDIA_TYPE: &str = "multipart/mixed";
const APPLICATION_HTTP: &str = "application/http";
const BINARY: &str = "binary";
const CRLF: &[u8] = b"\r\n";
const MAX_PART_HEADERS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BatchMediaVersion {
    MultipartMixed,
}

impl NegotiableMediaVersion for BatchMediaVersion {
    const SUPPORTED: &'static [Self] = &[Self::MultipartMixed];

    fn media_type(self) -> &'static str {
        match self {
            Self::MultipartMixed => BATCH_MEDIA_TYPE,
        }
    }
}

/// To be able to have a [`BatchOperation`] stick a bunch of operations
/// into a collection, we must be able to reference them by some common trait.
/// While they all implement [`Operation`], the associated response makes them
/// incompatible. So the response type must also be erased!
type ErasedResponse = Box<dyn Any + Send>;

trait ErasedOperation: Send + Sync {
    fn request(&self, client: &Client<Ready>) -> Result<AdtRequest, OperationError>;

    fn decode(&self, response: OperationResponse) -> Result<ErasedResponse, ResponseError>;
}

impl<O> ErasedOperation for O
where
    O: Operation<Ready> + 'static,
    O::Response: 'static,
{
    fn request(&self, client: &Client<Ready>) -> Result<AdtRequest, OperationError> {
        <O as Operation<Ready>>::request(self, client)
    }

    fn decode(&self, response: OperationResponse) -> Result<ErasedResponse, ResponseError> {
        <O as Operation<Ready>>::decode(self, response)
            .map(|response| Box::new(response) as ErasedResponse)
    }
}

/// A heterogeneous group of ADT operations executed in one HTTP round trip.
///
/// Every operation in a batch uses a [`Ready`] client and the same operation
/// kind `K`. Individual response types remain available through the [`BatchKey`]
/// returned by [`BatchOperation::push`]. The kind parameter prevents stateless
/// and stateful operations from being mixed. Stateless batches execute through
/// [`Client`], while stateful batches execute through [`super::UserSession`].
///
/// Construct this value through [`Client::batch`] or
/// [`super::UserSession::batch`]. ADT executes its subrequests and returns their
/// responses in request order.
pub struct BatchOperation<K: OperationKind> {
    identity: Arc<()>,
    endpoint: AdtUri,
    operations: Vec<Box<dyn ErasedOperation>>,
    kind: PhantomData<fn() -> K>,
}

impl<K> BatchOperation<K>
where
    K: OperationKind,
{
    /// Creates an empty batch using the endpoint advertised to a ready client.
    pub(crate) fn new(client: &Client<Ready>) -> Result<Self, CompatibilityError> {
        let collection = client.require_core_collection(BATCH_CATEGORY)?;
        BatchMediaVersion::negotiate(collection)?;

        Ok(Self {
            identity: Arc::new(()),
            endpoint: collection.target().clone(),
            operations: Vec::new(),
            kind: PhantomData,
        })
    }

    /// Returns the endpoint advertised for batch execution.
    pub fn endpoint(&self) -> &AdtUri {
        &self.endpoint
    }

    /// Returns the number of operations in this batch.
    pub fn len(&self) -> usize {
        self.operations.len()
    }

    /// Returns whether this batch contains no operations.
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Appends an operation and returns a typed key for its eventual response.
    ///
    /// See [`BatchKey`] for the magic this performs.
    pub fn push<O>(&mut self, operation: O) -> BatchKey<O::Response>
    where
        O: Operation<Ready, Kind = K> + 'static,
        O::Response: 'static,
    {
        let key = BatchKey {
            identity: Arc::clone(&self.identity),
            index: self.operations.len(),
            response: PhantomData::<fn() -> O::Response>,
        };
        self.operations.push(Box::new(operation));
        key
    }
}

impl<K> Operation<Ready> for BatchOperation<K>
where
    K: OperationKind,
{
    type Response = BatchResponses;
    type Kind = K;

    fn request(&self, client: &Client<Ready>) -> Result<AdtRequest, OperationError> {
        if self.operations.is_empty() {
            return Err(BatchError::Empty.into());
        }

        let mut requests = Vec::with_capacity(self.operations.len());
        let mut targets = Vec::with_capacity(self.operations.len());
        for operation in &self.operations {
            let request = operation.request(client)?;
            targets.push(request.target().clone());
            requests.push(request);
        }

        let boundary = format!("batch_{}", Uuid::new_v4());
        let mut request = AdtRequest::new(Method::POST, self.endpoint.clone());
        request.set_accept(BATCH_MEDIA_TYPE);
        request.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&format!("{BATCH_MEDIA_TYPE}; boundary={boundary}"))
                .expect("a UUID batch boundary is a valid Content-Type parameter"),
        );
        request.set_body(encode_batch(&requests, &boundary));
        request.set_response_context_targets(targets);
        Ok(request)
    }

    fn decode(&self, response: OperationResponse) -> Result<Self::Response, ResponseError> {
        let (response, context) = response.into_context_parts();
        if response.status() != StatusCode::ACCEPTED {
            return Err(ResponseError::UnexpectedStatus {
                status: response.status(),
                body: String::from_utf8_lossy(response.body()).into_owned(),
            });
        }

        let boundary = response_boundary(response.headers())?;
        let responses = decode_batch(response.body(), &boundary)?;
        let targets = context.related_request_targets();
        if targets.len() != self.operations.len() {
            return Err(BatchError::RequestContextCount {
                expected: self.operations.len(),
                actual: targets.len(),
            }
            .into());
        }
        if responses.len() != self.operations.len() {
            return Err(BatchError::ResponseCount {
                expected: self.operations.len(),
                actual: responses.len(),
            }
            .into());
        }

        let slots = self
            .operations
            .iter()
            .zip(targets.iter().cloned())
            .zip(responses)
            .map(|((operation, target), response)| {
                operation.decode(OperationResponse::new(response, target))
            })
            .map(Some)
            .collect();

        Ok(BatchResponses {
            identity: Arc::clone(&self.identity),
            slots,
        })
    }
}

/// A typed reference to one response slot in a batch.
pub struct BatchKey<R> {
    identity: Arc<()>,
    index: usize,
    response: PhantomData<fn() -> R>,
}

impl<R> Clone for BatchKey<R> {
    fn clone(&self) -> Self {
        Self {
            identity: Arc::clone(&self.identity),
            index: self.index,
            response: PhantomData,
        }
    }
}

/// Individually decoded responses from a heterogeneous batch.
#[derive(Debug)]
pub struct BatchResponses {
    identity: Arc<()>,
    slots: Vec<Option<Result<ErasedResponse, ResponseError>>>,
}

impl BatchResponses {
    /// Returns the total number of response slots in this batch.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Returns whether this batch has no response slots.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Takes and downcasts the response associated with `key`.
    ///
    /// A failed subrequest does not prevent other response slots from being
    /// retrieved. Its operation-specific decoding error is returned here.
    pub fn take<R>(&mut self, key: BatchKey<R>) -> Result<R, BatchError>
    where
        R: Send + 'static,
    {
        if !Arc::ptr_eq(&self.identity, &key.identity) {
            return Err(BatchError::ForeignBatch);
        }
        let slot = self
            .slots
            .get_mut(key.index)
            .ok_or(BatchError::MissingResponse { index: key.index })?
            .take()
            .ok_or(BatchError::MissingResponse { index: key.index })?;

        let response = slot.map_err(|source| BatchError::Decode {
            index: key.index,
            source: Box::new(source),
        })?;
        response
            .downcast::<R>()
            .map(|response| *response)
            .map_err(|_| BatchError::TypeMismatch { index: key.index })
    }
}

/// An error constructing, parsing, correlating, or retrieving a batch value.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BatchError {
    #[error("a batch must contain at least one operation")]
    Empty,

    #[error("batch response did not include a Content-Type header")]
    MissingContentType,

    #[error("batch response used unsupported Content-Type `{content_type}`")]
    UnsupportedContentType { content_type: String },

    #[error("batch Content-Type `{content_type}` did not include a boundary")]
    MissingBoundary { content_type: String },

    #[error("batch Content-Type contained an invalid boundary")]
    InvalidBoundary,

    #[error("invalid multipart batch response: {reason}")]
    InvalidMultipart { reason: String },

    #[error("invalid multipart batch response part {index}: {reason}")]
    InvalidPart { index: usize, reason: String },

    #[error("batch request context retained {actual} targets for {expected} operations")]
    RequestContextCount { expected: usize, actual: usize },

    #[error("batch returned {actual} response parts for {expected} operations")]
    ResponseCount { expected: usize, actual: usize },

    #[error("batch value belongs to a different batch operation")]
    ForeignBatch,

    #[error("batch response slot {index} is missing or was already taken")]
    MissingResponse { index: usize },

    #[error("batch response slot {index} could not be decoded: {source}")]
    Decode {
        index: usize,
        #[source]
        source: Box<ResponseError>,
    },

    #[error("batch response slot {index} did not contain its registered response type")]
    TypeMismatch { index: usize },
}

fn encode_batch(requests: &[AdtRequest], boundary: &str) -> Vec<u8> {
    let marker = format!("--{boundary}");
    let mut output = Vec::new();
    output.extend_from_slice(marker.as_bytes());
    output.extend_from_slice(CRLF);

    for (index, request) in requests.iter().enumerate() {
        output.extend_from_slice(b"Content-Type: application/http\r\n");
        output.extend_from_slice(b"content-transfer-encoding: binary\r\n\r\n");
        output.extend_from_slice(request.method().as_str().as_bytes());
        output.push(b' ');
        output.extend_from_slice(encoded_target(request).as_bytes());
        output.extend_from_slice(b" HTTP/1.1\r\n");

        for name in request.headers().keys() {
            for value in request.headers().get_all(name) {
                output.extend_from_slice(name.as_str().as_bytes());
                output.push(b':');
                output.extend_from_slice(value.as_bytes());
                output.extend_from_slice(CRLF);
            }
        }
        output.extend_from_slice(CRLF);

        if !request.body().is_empty() {
            output.extend_from_slice(request.body());
            output.extend_from_slice(CRLF);
        }

        output.extend_from_slice(marker.as_bytes());
        if index + 1 == requests.len() {
            output.extend_from_slice(b"--");
        } else {
            output.extend_from_slice(CRLF);
        }
    }

    output
}

fn encoded_target(request: &AdtRequest) -> String {
    let mut target = request.target().as_str().to_owned();
    if request.query().is_empty() {
        return target;
    }

    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (name, value) in request.query() {
        serializer.append_pair(name, value);
    }
    target.push('?');
    target.push_str(&serializer.finish());
    target
}

fn response_boundary(headers: &HeaderMap) -> Result<String, BatchError> {
    let value = headers
        .get(header::CONTENT_TYPE)
        .ok_or(BatchError::MissingContentType)?;
    let content_type = value
        .to_str()
        .map_err(|_| BatchError::UnsupportedContentType {
            content_type: String::from_utf8_lossy(value.as_bytes()).into_owned(),
        })?;
    let mut fields = content_type.split(';');
    let media_type = fields.next().unwrap_or_default().trim();
    if !media_type.eq_ignore_ascii_case(BATCH_MEDIA_TYPE) {
        return Err(BatchError::UnsupportedContentType {
            content_type: content_type.to_owned(),
        });
    }

    let boundary = fields.find_map(|field| {
        let (name, value) = field.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("boundary")
            .then(|| value.trim())
    });
    let Some(boundary) = boundary else {
        return Err(BatchError::MissingBoundary {
            content_type: content_type.to_owned(),
        });
    };
    let boundary = if boundary.starts_with('"') && boundary.ends_with('"') && boundary.len() >= 2 {
        &boundary[1..boundary.len() - 1]
    } else {
        boundary
    };
    if boundary.is_empty() || boundary.bytes().any(|byte| matches!(byte, b'\r' | b'\n')) {
        return Err(BatchError::InvalidBoundary);
    }
    Ok(boundary.to_owned())
}

fn decode_batch(body: &[u8], boundary: &str) -> Result<Vec<AdtResponse>, BatchError> {
    multipart_parts(body, boundary)?
        .into_iter()
        .enumerate()
        .map(|(index, part)| decode_part(index, part))
        .collect()
}

fn multipart_parts<'a>(body: &'a [u8], boundary: &str) -> Result<Vec<&'a [u8]>, BatchError> {
    let marker = format!("--{boundary}").into_bytes();
    let closing_marker = [marker.as_slice(), b"--"].concat();
    let mut parts = Vec::new();
    let mut part_start = None;
    let mut line_start = 0;

    loop {
        let line_end = find_bytes(&body[line_start..], CRLF)
            .map(|offset| line_start + offset)
            .unwrap_or(body.len());
        let next_line = (line_end < body.len()).then_some(line_end + CRLF.len());
        let line = &body[line_start..line_end];

        if line == marker {
            if let Some(start) = part_start {
                parts.push(&body[start..line_start]);
            }
            part_start = Some(next_line.ok_or_else(|| BatchError::InvalidMultipart {
                reason: "opening boundary was not followed by CRLF".to_owned(),
            })?);
        } else if line == closing_marker {
            let Some(start) = part_start else {
                return Err(BatchError::InvalidMultipart {
                    reason: "closing boundary appeared before an opening boundary".to_owned(),
                });
            };
            if start < line_start {
                parts.push(&body[start..line_start]);
            }
            return Ok(parts);
        }

        let Some(next_line) = next_line else {
            break;
        };
        line_start = next_line;
    }

    Err(BatchError::InvalidMultipart {
        reason: if part_start.is_some() {
            "closing boundary was not found".to_owned()
        } else {
            "opening boundary was not found".to_owned()
        },
    })
}

fn decode_part(index: usize, part: &[u8]) -> Result<AdtResponse, BatchError> {
    let (embedded_offset, meta_headers) = decode_mime_headers(index, part)?;
    let content_type = meta_headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::trim);
    if !content_type.is_some_and(|value| value.eq_ignore_ascii_case(APPLICATION_HTTP)) {
        return Err(invalid_part(
            index,
            "MIME Content-Type must be application/http",
        ));
    }
    if let Some(encoding) = meta_headers
        .get("content-transfer-encoding")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        && !encoding.eq_ignore_ascii_case(BINARY)
    {
        return Err(invalid_part(
            index,
            "MIME content-transfer-encoding must be binary",
        ));
    }

    let embedded_response = &part[embedded_offset..];
    let mut raw_headers = [httparse::EMPTY_HEADER; MAX_PART_HEADERS];
    let mut parsed = httparse::Response::new(&mut raw_headers);
    let response_offset = match parsed
        .parse(embedded_response)
        .map_err(|error| invalid_part(index, &format!("invalid embedded HTTP response: {error}")))?
    {
        httparse::Status::Complete(offset) => offset,
        httparse::Status::Partial => {
            return Err(invalid_part(index, "incomplete embedded HTTP response"));
        }
    };
    let status = parsed
        .code
        .and_then(|status| StatusCode::from_u16(status).ok())
        .ok_or_else(|| invalid_part(index, "invalid embedded HTTP status code"))?;
    let headers = decode_headers(index, parsed.headers)?;

    let mut response_body = &embedded_response[response_offset..];
    if let Some(body) = response_body.strip_suffix(CRLF) {
        response_body = body;
    }
    Ok(AdtResponse::new(status, headers, response_body.to_vec()))
}

fn decode_mime_headers(index: usize, part: &[u8]) -> Result<(usize, HeaderMap), BatchError> {
    let mut raw_headers = [httparse::EMPTY_HEADER; MAX_PART_HEADERS];
    let (offset, raw_headers) = match httparse::parse_headers(part, &mut raw_headers)
        .map_err(|error| invalid_part(index, &format!("invalid MIME headers: {error}")))?
    {
        httparse::Status::Complete(parsed) => parsed,
        httparse::Status::Partial => return Err(invalid_part(index, "incomplete MIME headers")),
    };
    Ok((offset, decode_headers(index, raw_headers)?))
}

fn decode_headers(
    index: usize,
    raw_headers: &[httparse::Header<'_>],
) -> Result<HeaderMap, BatchError> {
    let mut headers = HeaderMap::new();
    for raw_header in raw_headers {
        let name = HeaderName::from_bytes(raw_header.name.as_bytes())
            .map_err(|_| invalid_part(index, "invalid header name"))?;
        let value = HeaderValue::from_bytes(raw_header.value)
            .map_err(|_| invalid_part(index, "invalid header value"))?;
        headers.append(name, value);
    }
    Ok(headers)
}

fn invalid_part(index: usize, reason: &str) -> BatchError {
    BatchError::InvalidPart {
        index,
        reason: reason.to_owned(),
    }
}

fn find_bytes(value: &[u8], needle: &[u8]) -> Option<usize> {
    value
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex as StdMutex},
    };

    use async_trait::async_trait;

    use super::*;
    use crate::{
        Ready, Stateful, Stateless, Transport, TransportError, models::parse_capabilities,
    };

    const DISCOVERY_XML: &[u8] = include_bytes!("../../tests/fixtures/discovery.xml");
    const CORE_DISCOVERY_XML: &[u8] = include_bytes!("../../tests/fixtures/core-discovery.xml");
    const RESPONSE_BOUNDARY: &str = "batch_00112233445566778899AABBCCDDEEFF";

    struct TextOperation;

    impl Operation<Ready> for TextOperation {
        type Response = String;
        type Kind = Stateless;

        fn request(&self, _client: &Client<Ready>) -> Result<AdtRequest, OperationError> {
            let mut request =
                AdtRequest::new(Method::GET, AdtUri::parse("/sap/bc/adt/test/text").unwrap());
            request.push_query("name", "hello world");
            request
                .headers_mut()
                .insert(header::ACCEPT, HeaderValue::from_static("text/plain"));
            Ok(request)
        }

        fn decode(&self, response: OperationResponse) -> Result<Self::Response, ResponseError> {
            assert_eq!(response.request_target().as_str(), "/sap/bc/adt/test/text");
            expect_ok(response).map(|body| String::from_utf8_lossy(&body).into_owned())
        }
    }

    struct CountOperation;

    impl Operation<Ready> for CountOperation {
        type Response = usize;
        type Kind = Stateless;

        fn request(&self, _client: &Client<Ready>) -> Result<AdtRequest, OperationError> {
            Ok(AdtRequest::new(
                Method::GET,
                AdtUri::parse("/sap/bc/adt/test/count").unwrap(),
            ))
        }

        fn decode(&self, response: OperationResponse) -> Result<Self::Response, ResponseError> {
            assert_eq!(response.request_target().as_str(), "/sap/bc/adt/test/count");
            let body = expect_ok(response)?;
            Ok(String::from_utf8_lossy(&body).parse().unwrap())
        }
    }

    struct StatefulTextOperation;

    impl Operation<Ready> for StatefulTextOperation {
        type Response = String;
        type Kind = Stateful;

        fn request(&self, _client: &Client<Ready>) -> Result<AdtRequest, OperationError> {
            Ok(AdtRequest::new(
                Method::GET,
                AdtUri::parse("/sap/bc/adt/test/stateful").unwrap(),
            ))
        }

        fn decode(&self, response: OperationResponse) -> Result<Self::Response, ResponseError> {
            expect_ok(response).map(|body| String::from_utf8_lossy(&body).into_owned())
        }
    }

    fn expect_ok(response: OperationResponse) -> Result<Vec<u8>, ResponseError> {
        if response.status() == StatusCode::OK {
            Ok(response.into_body())
        } else {
            Err(ResponseError::UnexpectedStatus {
                status: response.status(),
                body: String::from_utf8_lossy(response.body()).into_owned(),
            })
        }
    }

    struct FixtureTransport {
        requests: Arc<StdMutex<Vec<AdtRequest>>>,
        responses: StdMutex<VecDeque<AdtResponse>>,
    }

    #[async_trait]
    impl Transport for FixtureTransport {
        async fn send(&self, request: AdtRequest) -> Result<AdtResponse, TransportError> {
            self.requests.lock().unwrap().push(request);
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| TransportError::new(std::io::Error::other("no fixture response")))
        }
    }

    fn fixture_response(parts: &[(&str, StatusCode)]) -> AdtResponse {
        fixture_response_with_headers(parts, HeaderMap::new())
    }

    fn fixture_response_with_headers(
        parts: &[(&str, StatusCode)],
        mut headers: HeaderMap,
    ) -> AdtResponse {
        let mut body = Vec::new();
        for (content, status) in parts {
            body.extend_from_slice(format!("--{RESPONSE_BOUNDARY}\r\n").as_bytes());
            body.extend_from_slice(b"content-type: application/http\r\n");
            body.extend_from_slice(b"content-transfer-encoding: binary\r\n\r\n");
            body.extend_from_slice(
                format!(
                    "HTTP/1.1 {} fixture\r\nContent-Type: text/plain\r\n\r\n",
                    status.as_u16()
                )
                .as_bytes(),
            );
            body.extend_from_slice(content.as_bytes());
            body.extend_from_slice(CRLF);
        }
        body.extend_from_slice(format!("--{RESPONSE_BOUNDARY}--\r\n").as_bytes());

        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&format!("multipart/mixed; boundary={RESPONSE_BOUNDARY}"))
                .unwrap(),
        );
        AdtResponse::new(StatusCode::ACCEPTED, headers, body)
    }

    fn fixture_client(
        responses: Vec<AdtResponse>,
    ) -> (Client<Ready>, Arc<StdMutex<Vec<AdtRequest>>>) {
        let requests = Arc::new(StdMutex::new(Vec::new()));
        let transport = FixtureTransport {
            requests: Arc::clone(&requests),
            responses: StdMutex::new(responses.into()),
        };
        let client = Client::new(transport).with_capabilities(
            parse_capabilities(DISCOVERY_XML).unwrap(),
            parse_capabilities(CORE_DISCOVERY_XML).unwrap(),
        );
        (client, requests)
    }

    #[tokio::test]
    async fn executes_and_decodes_heterogeneous_operations() {
        let (client, requests) = fixture_client(vec![fixture_response(&[
            ("hello", StatusCode::OK),
            ("42", StatusCode::OK),
        ])]);
        let mut batch = client.batch().unwrap();
        let text = batch.push(TextOperation);
        let count = batch.push(CountOperation);

        let mut responses = batch.execute(&client).await.unwrap();

        assert_eq!(responses.len(), 2);
        assert_eq!(responses.take(text).unwrap(), "hello");
        assert_eq!(responses.take(count).unwrap(), 42);

        let requests = requests.lock().unwrap();
        let request = &requests[0];
        assert_eq!(request.method(), Method::POST);
        assert_eq!(request.target(), batch.endpoint());
        assert_eq!(
            request.headers().get(header::ACCEPT).unwrap(),
            BATCH_MEDIA_TYPE
        );
        let boundary = response_boundary(request.headers()).unwrap();
        assert!(boundary.starts_with("batch_"));
        let body = String::from_utf8(request.body().to_vec()).unwrap();
        assert!(body.contains(&format!("--{boundary}\r\nContent-Type: application/http")));
        assert!(body.contains("GET /sap/bc/adt/test/text?name=hello+world HTTP/1.1\r\n"));
        assert!(body.contains("accept:text/plain\r\n\r\n"));
        assert!(body.contains("GET /sap/bc/adt/test/count HTTP/1.1\r\n\r\n"));
        assert!(body.ends_with(&format!("--{boundary}--")));
    }

    #[test]
    fn encodes_the_sap_application_http_contract() {
        let mut get = AdtRequest::new(Method::GET, AdtUri::parse("/sap/bc/adt/test/read").unwrap());
        get.push_query("name", "hello world");
        get.headers_mut()
            .insert(header::ACCEPT, HeaderValue::from_static("text/plain"));
        let mut post = AdtRequest::new(
            Method::POST,
            AdtUri::parse("/sap/bc/adt/test/write").unwrap(),
        );
        post.set_content_type("application/xml");
        post.set_body(b"<value/>".to_vec());

        let encoded = encode_batch(&[get, post], "batch_test");

        assert_eq!(
            encoded,
            b"--batch_test\r\n\
Content-Type: application/http\r\n\
content-transfer-encoding: binary\r\n\r\n\
GET /sap/bc/adt/test/read?name=hello+world HTTP/1.1\r\n\
accept:text/plain\r\n\r\n\
--batch_test\r\n\
Content-Type: application/http\r\n\
content-transfer-encoding: binary\r\n\r\n\
POST /sap/bc/adt/test/write HTTP/1.1\r\n\
content-type:application/xml\r\n\r\n\
<value/>\r\n\
--batch_test--"
        );
    }

    #[tokio::test]
    async fn rejects_an_empty_batch_before_transport() {
        let (client, requests) = fixture_client(Vec::new());
        let batch = client.batch().unwrap();

        let Err(error) = batch.execute(&client).await else {
            panic!("empty batch succeeded");
        };

        assert!(matches!(error, OperationError::Batch(BatchError::Empty)));
        assert!(requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn keeps_subrequest_failures_in_their_typed_slots() {
        let (client, _) = fixture_client(vec![fixture_response(&[
            ("missing", StatusCode::NOT_FOUND),
            ("7", StatusCode::OK),
        ])]);
        let mut batch = client.batch().unwrap();
        let failed = batch.push(TextOperation);
        let successful = batch.push(CountOperation);

        let mut responses = batch.execute(&client).await.unwrap();

        assert!(matches!(
            responses.take(failed),
            Err(BatchError::Decode { index: 0, .. })
        ));
        assert_eq!(responses.take(successful).unwrap(), 7);
    }

    #[tokio::test]
    async fn rejects_wrong_response_count() {
        let (client, _) = fixture_client(vec![fixture_response(&[("hello", StatusCode::OK)])]);
        let mut batch = client.batch().unwrap();
        batch.push(TextOperation);
        batch.push(CountOperation);

        let Err(error) = batch.execute(&client).await else {
            panic!("batch with a missing response part succeeded");
        };

        assert!(matches!(
            error,
            OperationError::Response(ResponseError::Batch(BatchError::ResponseCount {
                expected: 2,
                actual: 1,
            }))
        ));
    }

    #[tokio::test]
    async fn rejects_response_keys_from_another_batch() {
        let (client, _) = fixture_client(vec![fixture_response(&[("hello", StatusCode::OK)])]);
        let mut batch = client.batch().unwrap();
        let text = batch.push(TextOperation);
        let mut other = client.batch().unwrap();
        let foreign = other.push(TextOperation);
        let mut responses = batch.execute(&client).await.unwrap();

        assert!(matches!(
            responses.take(foreign),
            Err(BatchError::ForeignBatch)
        ));
        assert_eq!(responses.take(text).unwrap(), "hello");
    }

    #[tokio::test]
    async fn stateful_batch_uses_outer_user_session() {
        let mut first_headers = HeaderMap::new();
        first_headers.insert(
            header::SET_COOKIE,
            HeaderValue::from_static("sap-contextid=batch-context; Path=/sap/bc/adt"),
        );
        let first_response =
            fixture_response_with_headers(&[("first", StatusCode::OK)], first_headers);
        let second_response = fixture_response(&[("second", StatusCode::OK)]);
        let (client, requests) = fixture_client(vec![first_response, second_response]);
        let session = client.create_user_session();
        let mut batch = session.batch().unwrap();
        let first = batch.push(StatefulTextOperation);

        let mut responses = batch.execute(&session).await.unwrap();
        assert_eq!(responses.take(first).unwrap(), "first");
        batch.execute(&session).await.unwrap();

        let requests = requests.lock().unwrap();
        assert_eq!(
            requests[0]
                .headers()
                .get(super::super::ADT_SESSION_TYPE)
                .unwrap(),
            "stateful"
        );
        assert!(!requests[0].headers().contains_key(header::COOKIE));
        assert_eq!(
            requests[1].headers().get(header::COOKIE).unwrap(),
            "sap-contextid=batch-context"
        );
        assert!(!String::from_utf8_lossy(requests[1].body()).contains("sap-contextid"));
    }

    #[test]
    fn parses_quoted_boundary_and_preserves_body_line_ending() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("Multipart/Mixed; charset=utf-8; BOUNDARY=\"batch_test\""),
        );
        let boundary = response_boundary(&headers).unwrap();
        let body = b"--batch_test\r\nContent-Type: application/http\r\n\r\nHTTP/1.1 200 OK\r\nX-Test: one\r\nX-Test: two\r\n\r\nline\r\n\r\n--batch_test--\r\n";

        let responses = decode_batch(body, &boundary).unwrap();

        assert_eq!(responses[0].body(), b"line\r\n");
        assert_eq!(responses[0].headers().get_all("x-test").iter().count(), 2);
    }

    #[test]
    fn rejects_a_multipart_response_without_a_boundary() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/mixed"),
        );

        assert!(matches!(
            response_boundary(&headers),
            Err(BatchError::MissingBoundary { .. })
        ));
    }
}
