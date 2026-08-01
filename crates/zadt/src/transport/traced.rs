use std::{borrow::Cow, time::Instant};

use async_trait::async_trait;
use http::header;
use quick_xml::{Reader, Writer, events::Event};
use tracing::Instrument;

use crate::{AdtRequest, AdtResponse, Transport, TransportError};

/// A transport decorator that emits structured, redacted ADT call traces.
///
/// Query values and sensitive header values are not logged. Request and response
/// bodies are omitted unless explicitly enabled with
/// [`Traced::with_body_logging`]. Applications select where events are written
/// by installing a `tracing` subscriber appropriate for their CLI, language
/// server, or test environment.
#[derive(Clone, Debug)]
pub struct Traced<T> {
    inner: T,
    body_log_limit: Option<usize>,
}

impl<T> Traced<T> {
    /// Wraps a transport with structured ADT call tracing.
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            body_log_limit: None,
        }
    }

    /// Enables request and XML response body logging up to `max_bytes`.
    ///
    /// Request bodies with textual media types are logged as UTF-8. XML request
    /// and response bodies are indented when they are well formed. Bodies can
    /// contain source code or business data, so applications should only enable
    /// this for diagnostic output with an appropriate size limit.
    pub fn with_body_logging(mut self, max_bytes: usize) -> Self {
        self.body_log_limit = Some(max_bytes);
        self
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
        if let Some(max_bytes) = self.body_log_limit
            && tracing::enabled!(tracing::Level::DEBUG)
        {
            log_request_body(
                &span,
                request.body(),
                header_value(&request, header::CONTENT_TYPE),
                max_bytes,
            );
        }
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
                if let Some(max_bytes) = self.body_log_limit
                    && tracing::enabled!(tracing::Level::DEBUG)
                {
                    log_response_body(&span, response.body(), response_content_type, max_bytes);
                }
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

enum BodyLog<'a> {
    Text(Cow<'a, str>),
    Empty,
    TooLarge,
    Unsupported,
    InvalidUtf8,
}

fn log_request_body(span: &tracing::Span, body: &[u8], content_type: &str, max_bytes: usize) {
    match body_for_log(body, content_type, max_bytes, true) {
        BodyLog::Text(body) => {
            let body = indent_body(&body);
            tracing::debug!(parent: span, "ADT request body:\n{body}");
        }
        BodyLog::TooLarge => tracing::debug!(
            parent: span,
            body_bytes = body.len() as u64,
            max_bytes = max_bytes as u64,
            "ADT request body omitted because it exceeds the logging limit"
        ),
        BodyLog::InvalidUtf8 => {
            tracing::debug!(parent: span, "ADT request body omitted because it is not UTF-8");
        }
        BodyLog::Empty | BodyLog::Unsupported => {}
    }
}

fn log_response_body(span: &tracing::Span, body: &[u8], content_type: &str, max_bytes: usize) {
    match body_for_log(body, content_type, max_bytes, false) {
        BodyLog::Text(body) => {
            let body = indent_body(&body);
            tracing::debug!(parent: span, "ADT response body:\n{body}");
        }
        BodyLog::TooLarge => tracing::debug!(
            parent: span,
            body_bytes = body.len() as u64,
            max_bytes = max_bytes as u64,
            "ADT response body omitted because it exceeds the logging limit"
        ),
        BodyLog::InvalidUtf8 => {
            tracing::debug!(parent: span, "ADT response body omitted because it is not UTF-8");
        }
        BodyLog::Empty | BodyLog::Unsupported => {}
    }
}

fn body_for_log<'a>(
    body: &'a [u8],
    content_type: &str,
    max_bytes: usize,
    include_text: bool,
) -> BodyLog<'a> {
    if body.is_empty() {
        return BodyLog::Empty;
    }

    let xml = is_xml_media_type(content_type);
    if !(xml || include_text && is_text_media_type(content_type)) {
        return BodyLog::Unsupported;
    }
    if body.len() > max_bytes {
        return BodyLog::TooLarge;
    }

    let Ok(text) = std::str::from_utf8(body) else {
        return BodyLog::InvalidUtf8;
    };
    if !xml {
        return BodyLog::Text(Cow::Borrowed(text));
    }

    let Some(formatted) = pretty_xml(text) else {
        return BodyLog::Text(Cow::Borrowed(text));
    };
    if formatted.len() > max_bytes {
        BodyLog::Text(Cow::Borrowed(text))
    } else {
        BodyLog::Text(Cow::Owned(formatted))
    }
}

fn is_xml_media_type(content_type: &str) -> bool {
    let media_type = base_media_type(content_type);
    media_type.eq_ignore_ascii_case("application/xml")
        || media_type.eq_ignore_ascii_case("text/xml")
        || ends_with_ignore_ascii_case(media_type, "+xml")
}

fn is_text_media_type(content_type: &str) -> bool {
    let media_type = base_media_type(content_type);
    media_type
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("text/"))
        || media_type.eq_ignore_ascii_case("application/json")
        || media_type.eq_ignore_ascii_case("application/x-www-form-urlencoded")
        || ends_with_ignore_ascii_case(media_type, "+json")
}

fn base_media_type(content_type: &str) -> &str {
    content_type
        .split_once(';')
        .map_or(content_type, |(media_type, _)| media_type)
        .trim()
}

fn ends_with_ignore_ascii_case(value: &str, suffix: &str) -> bool {
    value
        .get(value.len().saturating_sub(suffix.len())..)
        .is_some_and(|ending| ending.eq_ignore_ascii_case(suffix))
}

fn indent_body(body: &str) -> String {
    body.lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn pretty_xml(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 4);

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(event) => writer.write_event(event).ok()?,
            Err(_) => return None,
        }
    }

    String::from_utf8(writer.into_inner()).ok()
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
        .traced()
        .with_body_logging(64 * 1024);
        let request = AdtRequest::new(
            Method::GET,
            AdtUri::parse("/sap/bc/adt/core/discovery").unwrap(),
        );

        let response = transport.send(request).await.unwrap();

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), b"response");
    }

    #[test]
    fn body_logging_is_disabled_by_default() {
        let transport = Traced::new(FixtureTransport {
            calls: Arc::new(AtomicUsize::new(0)),
        });

        assert_eq!(transport.body_log_limit, None);
        assert_eq!(transport.with_body_logging(1024).body_log_limit, Some(1024));
    }

    #[test]
    fn formats_vendor_xml_bodies() {
        let body = br#"<?xml version="1.0"?><root><child value="1">text</child></root>"#;
        let BodyLog::Text(formatted) = body_for_log(
            body,
            "application/vnd.sap.adt.example.v1+xml; charset=utf-8",
            1024,
            false,
        ) else {
            panic!("XML body was not logged");
        };

        assert!(formatted.contains("\n    <child value=\"1\">"));
        assert!(formatted.ends_with("\n</root>"));
        assert!(is_xml_media_type("APPLICATION/XML"));
        assert!(is_xml_media_type("Application/Example+XML"));
    }

    #[test]
    fn preserves_malformed_xml_for_diagnostics() {
        let body = b"<root><child></root>";
        let BodyLog::Text(formatted) = body_for_log(body, "application/xml", 1024, false) else {
            panic!("malformed XML body was not logged");
        };

        assert_eq!(formatted, String::from_utf8_lossy(body));
    }

    #[test]
    fn logs_text_requests_but_only_xml_responses() {
        let body = b"REPORT z_example.";

        assert!(matches!(
            body_for_log(body, "text/plain; charset=utf-8", 1024, true),
            BodyLog::Text(_)
        ));
        assert!(matches!(
            body_for_log(body, "text/plain; charset=utf-8", 1024, false),
            BodyLog::Unsupported
        ));
    }

    #[test]
    fn omits_oversized_and_non_utf8_bodies() {
        assert!(matches!(
            body_for_log(b"12345", "application/xml", 4, false),
            BodyLog::TooLarge
        ));
        assert!(matches!(
            body_for_log(&[0xff], "application/xml", 4, false),
            BodyLog::InvalidUtf8
        ));
    }

    #[test]
    fn indents_logged_bodies_away_from_the_event_prefix() {
        assert_eq!(
            indent_body("<root>\n    <child/>\n</root>"),
            "    <root>\n        <child/>\n    </root>"
        );
    }
}
