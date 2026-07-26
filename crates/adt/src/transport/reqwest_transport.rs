use std::sync::Arc;

use async_lock::Mutex;
use async_trait::async_trait;
use http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use reqwest::cookie::{CookieStore, Jar};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::{AdtRequest, AdtResponse, ReqwestTransportBuildError, Transport, TransportError};

const CSRF_TOKEN_HEADER: &str = "x-csrf-token";
const CSRF_FETCH: &str = "Fetch";
const ADT_SESSION_TYPE_HEADER: &str = "x-sap-adt-sessiontype";
const STATELESS_SESSION_TYPE: &str = "stateless";

/// An ADT transport backed by `reqwest`.
///
/// Each transport owns an RFC-aware cookie store seeded with the configured
/// SAP client and language. Cookies returned by the SAP destination are
/// retained according to their domain, path, security, and expiration rules.
///
/// ADT `sap-contextid` cookies are excluded because they belong to individual
/// [`UserSession`](crate::UserSession) values rather than the transport-wide
/// security session.
///
/// Before the first mutating request, the transport fetches a CSRF token from
/// core discovery and reuses it for subsequent requests in the same security
/// session.
pub struct ReqwestTransport {
    client: reqwest::Client,
    cookies: Arc<SapCookieStore>,
    csrf_token: Mutex<Option<HeaderValue>>,
    destination: Url,
    username: String,
    password: SecretString,
}

impl ReqwestTransport {
    pub fn builder() -> ReqwestTransportBuilder {
        ReqwestTransportBuilder::default()
    }
}

#[derive(Default)]
pub struct ReqwestTransportBuilder {
    destination: Option<String>,
    sap_client: Option<String>,
    language: Option<String>,
    username: Option<String>,
    password: Option<SecretString>,
}

impl ReqwestTransportBuilder {
    pub fn destination(mut self, destination: impl Into<String>) -> Self {
        self.destination = Some(destination.into());
        self
    }

    pub fn sap_client(mut self, sap_client: impl Into<String>) -> Self {
        self.sap_client = Some(sap_client.into());
        self
    }

    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    pub fn basic_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self.password = Some(SecretString::from(password.into()));
        self
    }

    pub fn build(self) -> Result<ReqwestTransport, ReqwestTransportBuildError> {
        let destination = self
            .destination
            .ok_or(ReqwestTransportBuildError::MissingField("destination"))?;
        let mut destination = Url::parse(&destination)?;
        if !matches!(destination.scheme(), "http" | "https") {
            return Err(ReqwestTransportBuildError::UnsupportedScheme);
        }
        if !destination.username().is_empty()
            || destination.password().is_some()
            || destination.query().is_some()
            || destination.fragment().is_some()
        {
            return Err(ReqwestTransportBuildError::InvalidDestinationComponents);
        }
        destination.set_path("/");

        let sap_client = self
            .sap_client
            .ok_or(ReqwestTransportBuildError::MissingField("sap_client"))?;
        let language = self
            .language
            .ok_or(ReqwestTransportBuildError::MissingField("language"))?;
        let user_context = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("sap-client", &sap_client)
            .append_pair("sap-language", &language)
            .finish();
        let cookie_store = Arc::new(SapCookieStore::new(&destination, &user_context));
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .cookie_provider(Arc::clone(&cookie_store))
            .build()?;

        Ok(ReqwestTransport {
            client,
            cookies: cookie_store,
            csrf_token: Mutex::new(None),
            destination,
            username: self
                .username
                .ok_or(ReqwestTransportBuildError::MissingField("username"))?,
            password: self
                .password
                .ok_or(ReqwestTransportBuildError::MissingField("password"))?,
        })
    }
}

#[async_trait]
impl Transport for ReqwestTransport {
    async fn send(&self, request: AdtRequest) -> Result<AdtResponse, TransportError> {
        let (method, target, query, mut headers, body) = request.into_parts();
        let url = request_url(&self.destination, &target, &query).map_err(TransportError::new)?;

        if requires_csrf_token(&method) && !headers.contains_key(CSRF_TOKEN_HEADER) {
            headers.insert(CSRF_TOKEN_HEADER, self.csrf_token().await?);
        }

        // Merge request-specific user-session cookies into the security session.
        merge_cookie_headers(&mut headers, self.cookies.cookies(&url))
            .map_err(TransportError::new)?;

        let response = self
            .client
            .request(method, url)
            .headers(headers)
            .basic_auth(&self.username, Some(self.password.expose_secret()))
            .body(body)
            .send()
            .await
            .map_err(TransportError::new)?;

        let status = response.status();
        let headers = response.headers().clone();
        self.remember_csrf_token(&headers).await;
        let body = response
            .bytes()
            .await
            .map_err(TransportError::new)?
            .to_vec();
        Ok(AdtResponse::new(status, headers, body))
    }
}

impl ReqwestTransport {
    async fn csrf_token(&self) -> Result<HeaderValue, TransportError> {
        let mut token = self.csrf_token.lock().await;
        if let Some(token) = token.as_ref() {
            return Ok(token.clone());
        }

        let fetched = self.fetch_csrf_token().await?;
        *token = Some(fetched.clone());
        Ok(fetched)
    }

    async fn fetch_csrf_token(&self) -> Result<HeaderValue, TransportError> {
        let url = self
            .destination
            .join("/sap/bc/adt/core/discovery")
            .map_err(TransportError::new)?;
        let mut headers = HeaderMap::new();
        headers.insert(CSRF_TOKEN_HEADER, HeaderValue::from_static(CSRF_FETCH));
        headers.insert(
            ADT_SESSION_TYPE_HEADER,
            HeaderValue::from_static(STATELESS_SESSION_TYPE),
        );
        merge_cookie_headers(&mut headers, self.cookies.cookies(&url))
            .map_err(TransportError::new)?;

        let response = self
            .client
            .get(url)
            .headers(headers)
            .basic_auth(&self.username, Some(self.password.expose_secret()))
            .send()
            .await
            .map_err(TransportError::new)?;
        let status = response.status();
        let token = response.headers().get(CSRF_TOKEN_HEADER).cloned();
        response.bytes().await.map_err(TransportError::new)?;

        if status != StatusCode::OK {
            return Err(TransportError::new(CsrfTokenError::UnexpectedStatus(
                status,
            )));
        }
        token.ok_or_else(|| TransportError::new(CsrfTokenError::MissingToken))
    }

    async fn remember_csrf_token(&self, headers: &HeaderMap) {
        let Some(token) = headers.get(CSRF_TOKEN_HEADER) else {
            return;
        };
        if !matches!(token.to_str(), Ok(value) if value.eq_ignore_ascii_case("required") || value.eq_ignore_ascii_case(CSRF_FETCH))
        {
            *self.csrf_token.lock().await = Some(token.clone());
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum CsrfTokenError {
    #[error("CSRF token request returned unexpected HTTP status {0}")]
    UnexpectedStatus(StatusCode),

    #[error("CSRF token response did not include x-csrf-token")]
    MissingToken,
}

fn requires_csrf_token(method: &Method) -> bool {
    !matches!(
        *method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    )
}

#[derive(Debug, Default)]
struct SapCookieStore {
    jar: Jar,
}

impl SapCookieStore {
    fn new(destination: &Url, user_context: &str) -> Self {
        let jar = Jar::default();
        jar.add_cookie_str(
            &format!("sap-usercontext={user_context}; Path=/"),
            destination,
        );
        Self { jar }
    }
}

impl CookieStore for SapCookieStore {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        let cookies = cookie_headers
            .filter(|header| !is_adt_context_cookie(header))
            .collect::<Vec<_>>();
        self.jar.set_cookies(&mut cookies.into_iter(), url);
    }

    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        self.jar.cookies(url)
    }
}

fn is_adt_context_cookie(header: &HeaderValue) -> bool {
    header
        .to_str()
        .ok()
        .and_then(|value| value.split_once('='))
        .is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case("sap-contextid"))
}

fn merge_cookie_headers(
    headers: &mut HeaderMap,
    session_cookies: Option<HeaderValue>,
) -> Result<(), http::header::InvalidHeaderValue> {
    let mut cookies = Vec::new();
    if let Some(session_cookies) = session_cookies {
        cookies.extend_from_slice(session_cookies.as_bytes());
    }
    for request_cookies in headers.get_all(header::COOKIE) {
        if !cookies.is_empty() {
            cookies.extend_from_slice(b"; ");
        }
        cookies.extend_from_slice(request_cookies.as_bytes());
    }
    if !cookies.is_empty() {
        headers.insert(header::COOKIE, HeaderValue::from_bytes(&cookies)?);
    }
    Ok(())
}

fn request_url(
    destination: &Url,
    target: &crate::AdtUri,
    query_parameters: &[(String, String)],
) -> Result<Url, url::ParseError> {
    let mut url = destination.join(target.as_str())?;
    if !query_parameters.is_empty() {
        let mut query = url.query_pairs_mut();
        query.extend_pairs(
            query_parameters
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        );
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AdtUri;
    use http::Method;

    #[test]
    fn request_without_query_has_no_empty_query_delimiter() {
        let destination = Url::parse("https://sap.example.test/").unwrap();
        let request = AdtRequest::new(
            Method::GET,
            AdtUri::parse("/sap/bc/adt/core/discovery").unwrap(),
        );

        let url = request_url(&destination, request.target(), request.query()).unwrap();

        assert_eq!(url.query(), None);
        assert_eq!(
            url.as_str(),
            "https://sap.example.test/sap/bc/adt/core/discovery"
        );
    }

    #[test]
    fn cookie_store_keeps_security_session_but_excludes_adt_context() {
        let destination = Url::parse("https://sap.example.test/").unwrap();
        let store = SapCookieStore::new(&destination, "sap-client=001&sap-language=EN");
        let session = HeaderValue::from_static("SAP_SESSIONID_A4H_001=session; Path=/");
        let context = HeaderValue::from_static("sap-contextid=context; Path=/sap/bc/adt");

        store.set_cookies(&mut [&session, &context].into_iter(), &destination);

        let cookies = store.cookies(&destination).unwrap();
        let cookies = cookies.to_str().unwrap();
        assert!(cookies.contains("sap-usercontext=sap-client=001&sap-language=EN"));
        assert!(cookies.contains("SAP_SESSIONID_A4H_001=session"));
        assert!(!cookies.contains("sap-contextid"));
    }

    #[test]
    fn merges_session_and_request_specific_cookies() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("sap-contextid=context"),
        );

        merge_cookie_headers(
            &mut headers,
            Some(HeaderValue::from_static(
                "sap-usercontext=sap-client=001; SAP_SESSIONID_A4H_001=session",
            )),
        )
        .unwrap();

        assert_eq!(
            headers.get(header::COOKIE).unwrap(),
            "sap-usercontext=sap-client=001; SAP_SESSIONID_A4H_001=session; sap-contextid=context"
        );
    }
}
