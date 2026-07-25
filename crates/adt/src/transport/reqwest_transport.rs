use async_trait::async_trait;
use http::{HeaderValue, header};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::{AdtRequest, AdtResponse, ReqwestTransportBuildError, Transport, TransportError};

/// An ADT transport backed by `reqwest`.
pub struct ReqwestTransport {
    client: reqwest::Client,
    destination: Url,
    sap_user_context: HeaderValue,
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

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
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
        let sap_user_context = HeaderValue::from_str(&format!("sap-usercontext={user_context}"))
            .expect("form URL encoding produces a valid cookie header value");

        Ok(ReqwestTransport {
            client,
            destination,
            sap_user_context,
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
        let url = request_url(&self.destination, &request).map_err(TransportError::new)?;
        let mut headers = request.headers().clone();
        headers.append(header::COOKIE, self.sap_user_context.clone());

        let response = self
            .client
            .request(request.method().clone(), url)
            .headers(headers)
            .basic_auth(&self.username, Some(self.password.expose_secret()))
            .body(request.body().to_vec())
            .send()
            .await
            .map_err(TransportError::new)?;

        let status = response.status();
        let headers = response.headers().clone();
        let body = response
            .bytes()
            .await
            .map_err(TransportError::new)?
            .to_vec();
        Ok(AdtResponse::new(status, headers, body))
    }
}

fn request_url(destination: &Url, request: &AdtRequest) -> Result<Url, url::ParseError> {
    let mut url = destination.join(request.target().as_str())?;
    if !request.query().is_empty() {
        let mut query = url.query_pairs_mut();
        query.extend_pairs(request.query().iter().map(|(name, value)| (name, value)));
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

        let url = request_url(&destination, &request).unwrap();

        assert_eq!(url.query(), None);
        assert_eq!(
            url.as_str(),
            "https://sap.example.test/sap/bc/adt/core/discovery"
        );
    }
}
