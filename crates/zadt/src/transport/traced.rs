use std::time::Instant;

use async_trait::async_trait;
use http::header;
use tracing::Instrument;

use crate::{AdtRequest, AdtResponse, Transport, TransportError};

/// A transport decorator that emits structured, redacted ADT call traces.
///
/// Request and response bodies, query values, and sensitive header values are
/// not logged. Media types are retained as safe protocol metadata. Applications
/// select where events are written by installing a `tracing` subscriber
/// appropriate for their CLI, language server, or test environment.
#[derive(Clone, Debug)]
pub struct Traced<T> {
    inner: T,
}

impl<T> Traced<T> {
    /// Wraps a transport with structured ADT call tracing.
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    /// Returns the wrapped transport.
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Consumes the decorator and returns the wrapped transport.
    pub fn into_inner(self) -> T {
        self.inner
    }
}

#[async_trait]
impl<T: Transport> Transport for Traced<T> {
    async fn send(&self, request: AdtRequest) -> Result<AdtResponse, TransportError> {
        let method = request.method().clone();
        let target = request.target().clone();
        let query_parameters = request.query().len() as u64;
        let request_body_bytes = request.body().len() as u64;
        let accept = header_value(&request, header::ACCEPT);
        let content_type = header_value(&request, header::CONTENT_TYPE);
        let started = Instant::now();
        let span = tracing::debug_span!(
            "adt.request",
            %method,
            %target,
            query_parameters,
            request_body_bytes,
            accept,
            content_type,
        );

        tracing::debug!(parent: &span, "ADT request started");
        let result = self.inner.send(request).instrument(span.clone()).await;
        let elapsed_ms = started.elapsed().as_millis() as u64;

        match &result {
            Ok(response) => {
                let response_content_type = response
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default();
                tracing::debug!(
                    parent: &span,
                    status = %response.status(),
                    response_body_bytes = response.body().len() as u64,
                    response_content_type,
                    elapsed_ms,
                    "ADT request completed"
                );
            }
            Err(error) => tracing::warn!(
                parent: &span,
                %error,
                elapsed_ms,
                "ADT request failed"
            ),
        }

        result
    }
}

fn header_value(request: &AdtRequest, name: header::HeaderName) -> &str {
    request
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use http::{HeaderMap, Method, StatusCode};

    use super::*;
    use crate::{AdtUri, TransportExt};

    #[derive(Clone)]
    struct FixtureTransport {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Transport for FixtureTransport {
        async fn send(&self, request: AdtRequest) -> Result<AdtResponse, TransportError> {
            assert_eq!(request.method(), Method::GET);
            assert_eq!(request.target().as_str(), "/sap/bc/adt/core/discovery");
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(AdtResponse::new(
                StatusCode::OK,
                HeaderMap::new(),
                b"response".to_vec(),
            ))
        }
    }

    #[tokio::test]
    async fn traced_transport_delegates_calls_and_preserves_responses() {
        let calls = Arc::new(AtomicUsize::new(0));
        let transport = FixtureTransport {
            calls: Arc::clone(&calls),
        }
        .traced();
        let request = AdtRequest::new(
            Method::GET,
            AdtUri::parse("/sap/bc/adt/core/discovery").unwrap(),
        );

        let response = transport.send(request).await.unwrap();

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), b"response");
    }
}
