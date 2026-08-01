use std::time::{SystemTime, UNIX_EPOCH};

use http::{HeaderValue, Method, StatusCode, header};

use crate::{
    AdtRequest, AdtResponse, AdtUri, Client, ClientState, LogonError, NegotiableMediaVersion,
    Operation, OperationError, ResponseError, SessionInformation, Stateless,
    models::parse_session_information,
    target::HTTP_SESSIONS,
    vocabulary::{
        CANCEL_ON_CLOSE_HEADER, LOAD_BALANCER_HEADER, PURPOSE_HEADER, SECURITY_SESSION_HEADER,
    },
};

/// Supported versions of the session information presentation
#[derive(Clone, Debug, Copy, Eq, PartialEq)]
pub struct SessionMediaVersion(&'static str);

impl SessionMediaVersion {
    pub const V3: Self = Self("application/vnd.sap.adt.core.http.session.v3+xml");
}

impl NegotiableMediaVersion for SessionMediaVersion {
    const SUPPORTED: &'static [Self] = &[Self::V3];

    fn media_type(self) -> &'static str {
        self.0
    }
}

/// Establishes an authenticated ADT HTTP security session.
///
/// This HTTP-specific operation returns the advertised [`SessionInformation`]
/// without changing client typestate.
#[derive(Clone, Copy, Debug, Default)]
pub struct Logon;

impl<S: ClientState> Operation<S> for Logon {
    type Response = SessionInformation;
    type Kind = Stateless;

    fn request(&self, _client: &Client<S>) -> Result<AdtRequest, OperationError> {
        let mut request = HTTP_SESSIONS.request(Method::GET);
        request.push_query("_", cache_buster());
        request.set_accept(SessionMediaVersion::V3.media_type());

        // TODO: These should probably be statically typed enums
        request
            .headers_mut()
            .insert(SECURITY_SESSION_HEADER, HeaderValue::from_static("create"));
        request
            .headers_mut()
            .insert(PURPOSE_HEADER, HeaderValue::from_static("logon"));
        request
            .headers_mut()
            .insert(LOAD_BALANCER_HEADER, HeaderValue::from_static("fetch"));
        request
            .headers_mut()
            .insert(CANCEL_ON_CLOSE_HEADER, HeaderValue::from_static("true"));
        Ok(request)
    }

    fn decode(
        &self,
        response: AdtResponse,
        _request_target: &AdtUri,
    ) -> Result<Self::Response, ResponseError> {
        if response.status() != StatusCode::OK {
            return Err(ResponseError::UnexpectedStatus {
                status: response.status(),
                body: String::from_utf8_lossy(response.body()).into_owned(),
            });
        }
        if response.body().is_empty() {
            return Err(LogonError::MissingResponseBody.into());
        }
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .ok_or(LogonError::MissingContentType)?;
        if SessionMediaVersion::from_media_type(content_type) != Some(SessionMediaVersion::V3) {
            return Err(LogonError::UnsupportedContentType {
                content_type: content_type.to_owned(),
            }
            .into());
        }
        parse_session_information(response.body()).map_err(Into::into)
    }
}

fn cache_buster() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string()
}
