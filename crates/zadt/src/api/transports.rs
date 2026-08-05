use derive_builder::Builder;
use http::{Method, StatusCode, header};

use crate::{
    AdtRequest, AdtUri, CategoryId, Client, CtsError, MediaVersionNegotiation, ObjectError,
    Operation, OperationError, OperationResponse, PostAction, Ready, ResponseError, Stateless,
    TransportCreation, TransportKind, TransportRequest, TransportRequests,
    models::TransportCreateRequest, target::CollectionTarget, vocabulary::query_parameter,
};

const TRANSPORTS_CATEGORY: CategoryId = CategoryId {
    scheme: "http://www.sap.com/adt/categories/cts",
    term: "transports",
};
const TRANSPORT_REQUESTS_MEDIA_TYPE: &str =
    "application/vnd.sap.as+xml; charset=UTF-8; dataname=com.sap.adt.CorrectionRequests";
const TRANSPORT_REQUEST_MEDIA_TYPE: &str =
    "application/vnd.sap.as+xml; charset=UTF-8; dataname=com.sap.adt.CorrectionRequest";
const TRANSPORT_CREATE_LEGACY_MEDIA_TYPE: &str =
    "application/vnd.sap.as+xml; charset=UTF-8; dataname=com.sap.adt.CreateCorrectionRequest";
const TRANSPORT_CREATE_V1_MEDIA_TYPE: &str =
    "application/vnd.sap.as+xml; charset=UTF-8; dataname=com.sap.adt.CreateCorrectionRequest.v1";
const TRANSPORT_CREATE_RESULT_MEDIA_TYPE: &str =
    "application/vnd.sap.as+xml; charset=UTF-8; dataname=com.sap.adt.CorrectionRequestResult";
const PLAIN_TEXT_MEDIA_TYPE: &str = "text/plain";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TransportRequestsMediaType;

impl MediaVersionNegotiation for TransportRequestsMediaType {
    const SUPPORTED: &'static [Self] = &[Self];

    fn media_type(self) -> &'static str {
        TRANSPORT_REQUESTS_MEDIA_TYPE
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TransportRequestMediaType;

impl MediaVersionNegotiation for TransportRequestMediaType {
    const SUPPORTED: &'static [Self] = &[Self];

    fn media_type(self) -> &'static str {
        TRANSPORT_REQUEST_MEDIA_TYPE
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TransportCreationMediaType;

impl MediaVersionNegotiation for TransportCreationMediaType {
    const SUPPORTED: &'static [Self] = &[Self];

    fn media_type(self) -> &'static str {
        TRANSPORT_CREATE_RESULT_MEDIA_TYPE
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransportCreateMediaVersion {
    V1,
    Legacy,
}

impl MediaVersionNegotiation for TransportCreateMediaVersion {
    const SUPPORTED: &'static [Self] = &[Self::V1, Self::Legacy];

    fn media_type(self) -> &'static str {
        match self {
            Self::V1 => TRANSPORT_CREATE_V1_MEDIA_TYPE,
            Self::Legacy => TRANSPORT_CREATE_LEGACY_MEDIA_TYPE,
        }
    }
}

impl TransportCreateMediaVersion {
    fn response_media_type(self) -> &'static str {
        match self {
            Self::V1 => TRANSPORT_CREATE_RESULT_MEDIA_TYPE,
            Self::Legacy => PLAIN_TEXT_MEDIA_TYPE,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PlainTextMediaType;

impl MediaVersionNegotiation for PlainTextMediaType {
    const SUPPORTED: &'static [Self] = &[Self];

    fn media_type(self) -> &'static str {
        PLAIN_TEXT_MEDIA_TYPE
    }
}

/// Transport functions included in a transport query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueryTransportKind {
    Kind(TransportKind),
    All,
}

impl QueryTransportKind {
    fn as_str(&self) -> &str {
        match self {
            Self::Kind(kind) => kind.as_str(),
            Self::All => "*",
        }
    }
}

impl From<TransportKind> for QueryTransportKind {
    fn from(kind: TransportKind) -> Self {
        Self::Kind(kind)
    }
}

/// Queries transports given a user and a transport kind.
///
/// This sends an `_action=FIND` GET request to the transports endpoint,
/// the response type is custom ABAP ASX.
///
/// If the user is omitted, the current user is used by the backend.
///
/// Backend handler: `CL_CTS_ADT_RES_OBJ_RECORD`
#[derive(Clone, Debug, Builder)]
#[builder(pattern = "owned", setter(into), default)]
pub struct TransportsQuery {
    /// The transport owner. The backend uses the current user when omitted.
    #[builder(setter(strip_option))]
    user: Option<String>,

    /// The transport functions to include.
    kind: QueryTransportKind,
}

impl Default for TransportsQuery {
    fn default() -> Self {
        Self {
            user: None,
            kind: QueryTransportKind::Kind(TransportKind::Workbench),
        }
    }
}

impl TransportsQuery {
    const TARGET: CollectionTarget = CollectionTarget::new(TRANSPORTS_CATEGORY);

    /// Creates a query for the current user's Workbench transports.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a configurable transport query.
    pub fn builder() -> TransportsQueryBuilder {
        TransportsQueryBuilder::default()
    }
}

impl Operation<Ready> for TransportsQuery {
    type Response = TransportRequests;
    type Kind = Stateless;

    fn request(&self, client: &Client<Ready>) -> Result<AdtRequest, OperationError> {
        let mut request = Self::TARGET.request(client, Method::GET)?;
        request.push_query(query_parameter::ACTION, PostAction::Find.as_str());
        if let Some(user) = &self.user {
            request.push_query("user", user);
        }
        request.push_query("trfunction", self.kind.as_str());
        request.set_accept(TRANSPORT_REQUESTS_MEDIA_TYPE);
        Ok(request)
    }

    fn decode(&self, response: OperationResponse) -> Result<Self::Response, ResponseError> {
        if response.status() != StatusCode::OK {
            return Err(ResponseError::UnexpectedStatus {
                status: response.status(),
                body: String::from_utf8_lossy(response.body()).into_owned(),
            });
        }
        if response.body().is_empty() {
            return Ok(TransportRequests::default());
        }

        let Some(content_type) = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
        else {
            return Err(ResponseError::MissingContentType {
                category: TRANSPORTS_CATEGORY,
            });
        };

        if TransportRequestsMediaType::from_media_type(content_type).is_none() {
            return Err(ResponseError::UnsupportedContentType {
                category: TRANSPORTS_CATEGORY,
                content_type: content_type.to_owned(),
                supported: vec![TRANSPORT_REQUESTS_MEDIA_TYPE.to_owned()],
            });
        }

        TransportRequests::parse(response.body()).map_err(Into::into)
    }
}

/// Fetches one CTS transport request by its transport number.
///
/// The backend returns an empty `200 OK` response when the transport does not
/// exist, represented by `None` in the operation response. This is handled
/// by the same endpoint as [`TransportsQuery`].
///
/// Backend handler: `CL_CTS_ADT_RES_OBJ_RECORD`
#[derive(Clone, Debug)]
pub struct TransportPropertiesQuery {
    transport_number: String,
}

impl TransportPropertiesQuery {
    const TARGET: CollectionTarget = CollectionTarget::new(TRANSPORTS_CATEGORY);

    /// Creates a query for one transport request.
    pub fn new(transport_number: impl Into<String>) -> Self {
        Self {
            transport_number: transport_number.into(),
        }
    }

    /// Returns the requested transport number.
    pub fn transport_number(&self) -> &str {
        &self.transport_number
    }
}

impl Operation<Ready> for TransportPropertiesQuery {
    type Response = Option<TransportRequest>;
    type Kind = Stateless;

    fn request(&self, client: &Client<Ready>) -> Result<AdtRequest, OperationError> {
        let collection = Self::TARGET.collection(client)?;
        let target = collection
            .target()
            .append_segments([&self.transport_number])
            .map_err(|source| ResponseError::Object(ObjectError::InvalidTarget(source)))?;
        let mut request = AdtRequest::new(Method::GET, target);
        request.set_accept(TRANSPORT_REQUEST_MEDIA_TYPE);
        Ok(request)
    }

    fn decode(&self, response: OperationResponse) -> Result<Self::Response, ResponseError> {
        if response.status() != StatusCode::OK {
            return Err(ResponseError::UnexpectedStatus {
                status: response.status(),
                body: String::from_utf8_lossy(response.body()).into_owned(),
            });
        }
        if response.body().is_empty() {
            return Ok(None);
        }

        let Some(content_type) = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
        else {
            return Err(ResponseError::MissingContentType {
                category: TRANSPORTS_CATEGORY,
            });
        };

        if TransportRequestMediaType::from_media_type(content_type).is_none() {
            return Err(ResponseError::UnsupportedContentType {
                category: TRANSPORTS_CATEGORY,
                content_type: content_type.to_owned(),
                supported: vec![TRANSPORT_REQUEST_MEDIA_TYPE.to_owned()],
            });
        }

        TransportRequest::parse(response.body())
            .map(Some)
            .map_err(Into::into)
    }
}

impl TransportRequest {
    /// Creates a query that refreshes this transport request's properties.
    pub fn properties_query(&self) -> TransportPropertiesQuery {
        TransportPropertiesQuery::new(self.number.clone())
    }
}

/// Creates a CTS transport request.
///
/// The modern ASX contract is preferred when advertised by discovery, with a
/// fallback to the legacy contract. The backend determines whether the created
/// request is Workbench or Customizing from the referenced object.
///
/// Backend handler: `CL_CTS_ADT_RES_OBJ_RECORD`
#[derive(Builder, Clone, Debug)]
#[builder(setter(into))]
pub struct TransportCreate {
    /// The transport description.
    description: String,

    /// The package (`DEVCLASS`) used to determine the transport target.
    #[builder(default, setter(strip_option))]
    package: Option<String>,

    /// An optional ADT resource that determines the request type and package.
    #[builder(default, setter(strip_option))]
    reference: Option<AdtUri>,

    /// A transport layer used when creating a transport for a new package.
    #[builder(default, setter(strip_option))]
    transport_layer: Option<String>,
}

impl TransportCreate {
    const TARGET: CollectionTarget = CollectionTarget::new(TRANSPORTS_CATEGORY);

    /// Creates a configurable transport request builder.
    pub fn builder() -> TransportCreateBuilder {
        TransportCreateBuilder::default()
    }
}

impl Operation<Ready> for TransportCreate {
    type Response = TransportCreation;
    type Kind = Stateless;

    fn request(&self, client: &Client<Ready>) -> Result<AdtRequest, OperationError> {
        let collection = Self::TARGET.collection(client)?;
        let media_version = TransportCreateMediaVersion::negotiate(collection)?;
        let body = TransportCreateRequest::new(
            self.package.as_deref(),
            &self.description,
            self.reference.as_ref(),
        )
        .serialize()?;

        let mut request = AdtRequest::new(Method::POST, collection.target().clone());
        if let Some(transport_layer) = &self.transport_layer {
            request.push_query("transportLayer", transport_layer);
        }
        request.set_accept(media_version.response_media_type());
        request.set_content_type(media_version.media_type());
        request.set_body(body);
        Ok(request)
    }

    fn decode(&self, response: OperationResponse) -> Result<Self::Response, ResponseError> {
        if !response.status().is_success() {
            return Err(ResponseError::UnexpectedStatus {
                status: response.status(),
                body: String::from_utf8_lossy(response.body()).into_owned(),
            });
        }
        if response.body().is_empty() {
            return Err(CtsError::MissingTransportCreationResponse.into());
        }

        let Some(content_type) = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
        else {
            return Err(ResponseError::MissingContentType {
                category: TRANSPORTS_CATEGORY,
            });
        };

        if TransportCreationMediaType::from_media_type(content_type).is_some() {
            TransportCreation::parse(response.body()).map_err(Into::into)
        } else if PlainTextMediaType::from_media_type(content_type).is_some() {
            TransportCreation::parse_legacy(response.body()).map_err(Into::into)
        } else {
            Err(ResponseError::UnsupportedContentType {
                category: TRANSPORTS_CATEGORY,
                content_type: content_type.to_owned(),
                supported: vec![
                    TRANSPORT_CREATE_RESULT_MEDIA_TYPE.to_owned(),
                    PLAIN_TEXT_MEDIA_TYPE.to_owned(),
                ],
            })
        }
    }
}
