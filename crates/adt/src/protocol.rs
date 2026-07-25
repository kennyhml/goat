use http::{HeaderMap, Method, StatusCode};

use crate::AdtUri;

/// A transport-neutral request to an ADT resource.
///
/// Different transports preserve ADT's HTTP-like method, target, query,
/// headers, and body semantics. They do not need to tunnel a serialized raw
/// HTTP message.
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

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn set_body(&mut self, body: impl Into<Vec<u8>>) {
        self.body = body.into();
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
