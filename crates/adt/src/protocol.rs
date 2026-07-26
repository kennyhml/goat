use std::{fmt, ops::Deref, str::FromStr};

use http::{
    HeaderMap, HeaderValue, Method, StatusCode,
    header::{self, InvalidHeaderValue},
};

use crate::AdtUri;

/// An entity tag validated for use as an HTTP header value.
///
/// This guarantees header safety but does not enforce the complete HTTP ETag
/// grammar, preserving the unquoted values emitted by some SAP systems.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EntityTag(HeaderValue);

impl EntityTag {
    /// Creates an entity tag from a static header value.
    pub fn from_static(value: &'static str) -> Self {
        Self(HeaderValue::from_static(value))
    }

    /// Returns the entity tag as text.
    pub fn as_str(&self) -> &str {
        self.0
            .to_str()
            .expect("an EntityTag always contains visible header text")
    }

    /// Returns the validated HTTP header value.
    pub fn as_header_value(&self) -> &HeaderValue {
        &self.0
    }

    pub(crate) fn from_header_value(value: &HeaderValue) -> Option<Self> {
        value.to_str().ok()?;
        Some(Self(value.clone()))
    }
}

impl FromStr for EntityTag {
    type Err = InvalidHeaderValue;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        HeaderValue::from_str(value).map(Self)
    }
}

impl TryFrom<String> for EntityTag {
    type Error = InvalidHeaderValue;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl TryFrom<&str> for EntityTag {
    type Error = InvalidHeaderValue;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl Deref for EntityTag {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for EntityTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PartialEq<str> for EntityTag {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<EntityTag> for str {
    fn eq(&self, other: &EntityTag) -> bool {
        self == other.as_str()
    }
}

/// A transport agnostic request to an ADT resource.
///
/// Different transports preserve the HTTP-like method, target, query,
/// headers, and body semantics. They do not need to tunnel a serialized raw
/// HTTP message.
///
/// For instance, Eclipse still uses RFC connections for on premise systems
/// that simply wrap the HTTP payload. This can be observed in the ABAP
/// communcation Log.
#[derive(Debug)]
pub struct AdtRequest {
    method: Method,
    target: AdtUri,
    query: Vec<(String, String)>,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl AdtRequest {
    pub fn new(method: Method, target: AdtUri) -> Self {
        Self {
            method,
            target,
            query: Vec::new(),
            headers: HeaderMap::new(),
            body: Vec::new(),
        }
    }

    pub fn method(&self) -> &Method {
        &self.method
    }

    pub fn target(&self) -> &AdtUri {
        &self.target
    }

    pub fn query(&self) -> &[(String, String)] {
        &self.query
    }

    pub fn push_query(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.query.push((name.into(), value.into()));
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }

    /// Sets the media type accepted for the response.
    pub fn set_accept(&mut self, media_type: &'static str) {
        self.headers
            .insert(header::ACCEPT, HeaderValue::from_static(media_type));
    }

    /// Sets the media type of the request body.
    pub fn set_content_type(&mut self, media_type: &'static str) {
        self.headers
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(media_type));
    }

    /// Configures cache revalidation with an ETag or an unconditional refresh.
    pub fn set_cache_revalidation(&mut self, if_none_match: Option<&EntityTag>) {
        if let Some(etag) = if_none_match {
            self.headers.remove(header::CACHE_CONTROL);
            self.headers
                .insert(header::IF_NONE_MATCH, etag.as_header_value().clone());
        } else {
            self.headers.remove(header::IF_NONE_MATCH);
            self.headers
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        }
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn set_body(&mut self, body: impl Into<Vec<u8>>) {
        self.body = body.into();
    }

    /// Consumes the request and returns its transport-level components.
    pub fn into_parts(self) -> (Method, AdtUri, Vec<(String, String)>, HeaderMap, Vec<u8>) {
        (
            self.method,
            self.target,
            self.query,
            self.headers,
            self.body,
        )
    }
}

/// A raw response returned by an ADT transport.
#[derive(Debug)]
pub struct AdtResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl AdtResponse {
    pub fn new(status: StatusCode, headers: HeaderMap, body: Vec<u8>) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn into_body(self) -> Vec<u8> {
        self.body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_tags_validate_header_safety_at_construction() {
        assert_eq!(
            EntityTag::try_from("safe-etag").unwrap().as_str(),
            "safe-etag"
        );
        assert!(EntityTag::try_from("etag\r\ninjected: value").is_err());
    }
}
