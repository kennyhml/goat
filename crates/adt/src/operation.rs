use std::future::Future;

use async_lock::Mutex;
use http::{HeaderMap, HeaderValue, header};
use secrecy::{ExposeSecret, SecretString};

use crate::{
    AdtRequest, AdtResponse, Client, ClientState, EntityTag, LoggedOnState, OperationError,
    ResponseError, TransportError,
};

const ADT_SESSION_TYPE: &str = "x-sap-adt-sessiontype";
const STATEFUL_SESSION_TYPE: &str = "stateful";
const STATELESS_SESSION_TYPE: &str = "stateless";
const USER_SESSION_COOKIE: &str = "sap-contextid";

mod private {
    pub trait Sealed {}
}

/// Identifies whether an ADT operation is [`Stateless`] or [`Stateful`].
///
/// Stateless operations do not require a persistent ABAP user session. They may
/// still use authentication and an HTTP security session.
///
/// Stateful operations execute within a [`UserSession`] retained across requests.
/// For example, updating a program requires a lock acquired and used within the
/// same user session. The session keeps the lock alive until it is released,
/// closed, or expires.
///
/// SAP exposes these user sessions in transaction `SM04`. For HTTP ADT, the
/// session is identified by the `sap-contextid` cookie. It is distinct from the
/// HTTP security session and from the `sap-usercontext` cookie used to select
/// the SAP client and language.
pub trait OperationKind: private::Sealed + Send + Sync {}

/// An operation that does not require a persistent ABAP user session.
#[derive(Debug)]
pub struct Stateless;

/// An operation that requires a persistent ABAP user session.
#[derive(Debug)]
pub struct Stateful;

impl private::Sealed for Stateless {}
impl private::Sealed for Stateful {}
impl OperationKind for Stateless {}
impl OperationKind for Stateful {}

/// A typed ADT operation possible only with client state `S`.
///
/// ADT uses HTTP resource semantics, including methods such as `GET`, `POST`,
/// and `PUT`, resource URIs, headers, and representation bodies.
///
/// [`AdtRequest`] represents those semantics independently of the transport.
/// An HTTP transport sends them as an HTTP request, while an RFC transport can
/// map the same fields into SAP's `SADT_REST_REQUEST` structure. It does not
/// tunnel a serialized raw HTTP message.
///
/// The operation's [`OperationKind`] and the client state determine which
/// [`Executor`] can run it.
///
/// Consumers of the API should construct operations manually only in exceptional
/// cases. In most scenarios, a callable operation can be constructed - or at least
/// partially derived - from an existing context, such as an object reference.
pub trait Operation<S: ClientState>: Send + Sync {
    type Response: Send;
    type Kind: OperationKind;

    /// Builds the transport-neutral request for this operation.
    fn request(&self, client: &Client<S>) -> Result<AdtRequest, OperationError>;

    /// Converts the raw transport response into this operation's response type.
    fn decode(&self, response: AdtResponse) -> Result<Self::Response, ResponseError>;

    /// Convenient forward of [`Executor::execute`] to the operation itself
    fn execute<E>(
        &self,
        executor: &E,
    ) -> impl Future<Output = Result<Self::Response, OperationError>> + Send
    where
        E: Executor<S, Self>,
        Self: Sized,
    {
        executor.execute(self)
    }
}

/// An execution context capable of running operation `O` with client state `S`.
///
/// `Operation` describes how to build and decode a request, while `Executor`
/// controls how that request is carried out. This separates the operations
/// protocol contract from execution concerns such as user-session affinity,
/// session headers, serialization, and transport access.
///
/// The generic parameters express two independent requirements:
///
/// - `S` is the discovery state needed to build the operations request.
/// - `O` is the concrete operation and determines response and [`OperationKind`].
///
/// [`Client<S>`](Client) implements this trait only for [`Stateless`]
/// operations. Consequently, a [`Stateful`] operation cannot execute directly
/// through a client. A [`UserSession`] implements this trait while retaining
/// the required `sap-contextid` and delegating request delivery to its client.
///
/// Callers should use [`Operation::execute`] rather than invoking this directly.
pub trait Executor<S, O>: Send + Sync
where
    S: ClientState,
    O: Operation<S>,
{
    /// Builds, sends, and decodes one operation within this execution context.
    fn execute(
        &self,
        operation: &O,
    ) -> impl Future<Output = Result<O::Response, OperationError>> + Send;
}

/// A long-lived SAP user session for stateful ADT operations.
///
/// SAP calls the stateful ABAP context represented by `sap-contextid` a user
/// session. Active user sessions can be inspected in transaction `SM04`. Do
/// not confuse this with the transports HTTP security session, identified by
/// `SAP_SESSIONID_*`, or the `sap-usercontext` client/language cookie.
///
/// The session owns a cheap clone of its [`Client`], so it has no borrowing
/// lifetime and can be retained for an entire editing workflow. Client
/// capabilities and the underlying transport remain shared. Requests within
/// one session are serialized, while separate sessions can hold independent
/// `sap-contextid` values.
///
/// A user session can retain locks and other server resources. Call
/// [`UserSession::close`] when the workflow finishes; dropping this value only
/// releases local state and does not notify SAP.
pub struct UserSession<S: LoggedOnState> {
    client: Client<S>,
    state: Mutex<UserSessionState>,
}

// Execution of a stateless request
impl<S, O> Executor<S, O> for Client<S>
where
    S: ClientState,
    O: Operation<S, Kind = Stateless>,
{
    async fn execute(&self, operation: &O) -> Result<O::Response, OperationError> {
        let request = operation.request(self)?;
        let response = self.transport().send(request).await?;
        Ok(operation.decode(response)?)
    }
}

// Execution of a stateful request
impl<S, O> Executor<S, O> for UserSession<S>
where
    S: LoggedOnState,
    O: Operation<S, Kind = Stateful>,
{
    async fn execute(&self, operation: &O) -> Result<O::Response, OperationError> {
        let mut session = self.state.lock().await;
        let mut request = operation.request(&self.client)?;
        session.decorate(&mut request)?;
        let response = self.client.transport().send(request).await?;
        session.update(response.headers());
        Ok(operation.decode(response)?)
    }
}

/// The outcome of a request using a cache validator such as `If-None-Match`.
#[derive(Clone, Debug)]
pub enum Conditional<T> {
    /// The resource changed and a new representation was returned.
    Modified(T),

    /// The supplied validator still identifies the current representation.
    NotModified { etag: Option<EntityTag> },
}

impl<T> Conditional<T> {
    /// Borrows the returned representation when the resource was modified.
    pub fn as_modified(&self) -> Option<&T> {
        match self {
            Self::Modified(value) => Some(value),
            Self::NotModified { .. } => None,
        }
    }

    /// Consumes the outcome and returns the representation when modified.
    pub fn into_modified(self) -> Option<T> {
        match self {
            Self::Modified(value) => Some(value),
            Self::NotModified { .. } => None,
        }
    }

    /// Returns the ETag supplied with a not-modified response.
    pub fn not_modified_etag(&self) -> Option<&str> {
        match self {
            Self::Modified(_) => None,
            Self::NotModified { etag } => etag.as_deref(),
        }
    }
}

/// Marker for a query without an HTTP cache validator.
#[derive(Clone, Copy, Debug, Default)]
pub struct Unconditional;

/// Marker for a query carrying an `If-None-Match` validator.
#[derive(Clone, Debug)]
pub struct IfNoneMatch {
    pub(crate) etag: EntityTag,
}

impl private::Sealed for Unconditional {}
impl private::Sealed for IfNoneMatch {}

/// Selects the response produced by a cache-aware query mode.
///
/// This trait is sealed; use a query's `if_none_match` method to change modes.
#[doc(hidden)]
pub trait QueryMode<V>: private::Sealed + Send + Sync {
    type Response: Send;

    fn if_none_match(&self) -> Option<&EntityTag>;
    fn modified(&self, value: V) -> Self::Response;
    fn not_modified(&self, etag: Option<EntityTag>) -> Option<Self::Response>;
}

impl<V: Send> QueryMode<V> for Unconditional {
    type Response = V;

    fn if_none_match(&self) -> Option<&EntityTag> {
        None
    }

    fn modified(&self, value: V) -> Self::Response {
        value
    }

    fn not_modified(&self, _etag: Option<EntityTag>) -> Option<Self::Response> {
        None
    }
}

impl<V: Send> QueryMode<V> for IfNoneMatch {
    type Response = Conditional<V>;

    fn if_none_match(&self) -> Option<&EntityTag> {
        Some(&self.etag)
    }

    fn modified(&self, value: V) -> Self::Response {
        Conditional::Modified(value)
    }

    fn not_modified(&self, etag: Option<EntityTag>) -> Option<Self::Response> {
        Some(Conditional::NotModified { etag })
    }
}

#[derive(Default)]
struct UserSessionState {
    context_id: Option<SecretString>,
}

impl UserSessionState {
    // Attaches the internal session id cookie to the request headers to be
    // merged by the transport layer later on if needed.
    fn decorate(&self, request: &mut AdtRequest) -> Result<(), TransportError> {
        request.headers_mut().insert(
            ADT_SESSION_TYPE,
            HeaderValue::from_static(STATEFUL_SESSION_TYPE),
        );
        if let Some(cookie) = self.cookie_header()? {
            request.headers_mut().append(header::COOKIE, cookie);
        }
        Ok(())
    }

    fn cookie_header(&self) -> Result<Option<HeaderValue>, TransportError> {
        self.context_id
            .as_ref()
            .map(|context_id| {
                HeaderValue::from_str(&format!(
                    "{USER_SESSION_COOKIE}={}",
                    context_id.expose_secret()
                ))
                .map_err(TransportError::new)
            })
            .transpose()
    }

    // Updates the session id based on the response. This may mean discarding the session
    // if it has expired, or setting / renewing it.
    fn update(&mut self, headers: &HeaderMap) {
        for header in headers.get_all(header::SET_COOKIE) {
            let Some(cookie) = header
                .to_str()
                .ok()
                .and_then(|value| cookie::Cookie::parse(value.to_owned()).ok())
                .filter(|cookie| cookie.name().eq_ignore_ascii_case(USER_SESSION_COOKIE))
            else {
                continue;
            };

            let expired = cookie.value_trimmed().is_empty()
                || cookie
                    .max_age()
                    .is_some_and(|duration| duration.whole_seconds() <= 0);
            self.context_id =
                (!expired).then(|| SecretString::from(cookie.value_trimmed().to_owned()));
        }
    }
}

impl<S> UserSession<S>
where
    S: LoggedOnState,
{
    pub(crate) fn new(client: Client<S>) -> Self {
        Self {
            client,
            state: Mutex::new(UserSessionState::default()),
        }
    }

    /// Returns the client whose capabilities and transport this session uses.
    pub fn client(&self) -> &Client<S> {
        &self.client
    }

    /// Closes this SAP user session and releases its server-side resources.
    ///
    /// If no stateful response established a `sap-contextid`, this returns
    /// without sending a request. Otherwise it performs a safe core-discovery
    /// request carrying the context with `x-sap-adt-sessiontype: stateless`,
    /// leaving the stateful backend session through an existing resource.
    pub async fn close(self) -> Result<(), OperationError> {
        let state = self.state.into_inner();
        let Some(cookie) = state.cookie_header()? else {
            return Ok(());
        };
        let target = crate::AdtUri::parse("/sap/bc/adt/core/discovery")
            .expect("the static core-discovery URI is valid");
        let mut request = AdtRequest::new(http::Method::GET, target);
        request.headers_mut().insert(
            ADT_SESSION_TYPE,
            HeaderValue::from_static(STATELESS_SESSION_TYPE),
        );
        request.headers_mut().append(header::COOKIE, cookie);
        let response = self.client.transport().send(request).await?;
        if response.status() == http::StatusCode::OK {
            Ok(())
        } else {
            Err(ResponseError::UnexpectedStatus {
                status: response.status(),
                body: String::from_utf8_lossy(response.body()).into_owned(),
            }
            .into())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex as StdMutex},
    };

    use async_trait::async_trait;
    use http::{HeaderMap, Method, StatusCode};

    use super::*;
    use crate::{AdtUri, LoggedOn, Transport, models::parse_session_information};

    const SESSION_XML: &[u8] = include_bytes!("../tests/fixtures/http-session-v3.xml");

    struct StatefulProbe;

    impl Operation<LoggedOn> for StatefulProbe {
        type Response = ();
        type Kind = Stateful;

        fn request(&self, _client: &Client<LoggedOn>) -> Result<AdtRequest, OperationError> {
            Ok(AdtRequest::new(
                Method::GET,
                AdtUri::parse("/sap/bc/adt/stateful-probe").unwrap(),
            ))
        }

        fn decode(&self, response: AdtResponse) -> Result<Self::Response, ResponseError> {
            if response.status() == StatusCode::OK {
                Ok(())
            } else {
                Err(ResponseError::UnexpectedStatus {
                    status: response.status(),
                    body: String::from_utf8_lossy(response.body()).into_owned(),
                })
            }
        }
    }

    struct ContextFixtureTransport {
        requests: Arc<StdMutex<Vec<HeaderMap>>>,
        responses: StdMutex<VecDeque<AdtResponse>>,
    }

    #[async_trait]
    impl Transport for ContextFixtureTransport {
        async fn send(&self, request: AdtRequest) -> Result<AdtResponse, TransportError> {
            self.requests
                .lock()
                .unwrap()
                .push(request.headers().clone());
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| TransportError::new(std::io::Error::other("no fixture response")))
        }
    }

    #[tokio::test]
    async fn user_session_is_owned_and_reuses_its_context_id() {
        let requests = Arc::new(StdMutex::new(Vec::new()));
        let mut context_headers = HeaderMap::new();
        context_headers.insert(
            header::SET_COOKIE,
            HeaderValue::from_static("sap-contextid=context-1; Path=/sap/bc/adt"),
        );
        let transport = ContextFixtureTransport {
            requests: Arc::clone(&requests),
            responses: StdMutex::new(VecDeque::from([
                AdtResponse::new(StatusCode::OK, context_headers, Vec::new()),
                AdtResponse::new(StatusCode::OK, HeaderMap::new(), Vec::new()),
            ])),
        };
        let session = Client::new(transport)
            .with_session_information(parse_session_information(SESSION_XML).unwrap())
            .create_user_session();

        fn assert_static<T: 'static>(_value: &T) {}
        assert_static(&session);
        StatefulProbe.execute(&session).await.unwrap();
        StatefulProbe.execute(&session).await.unwrap();

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].get(ADT_SESSION_TYPE).unwrap(),
            STATEFUL_SESSION_TYPE
        );
        assert!(!requests[0].contains_key(header::COOKIE));
        assert_eq!(
            requests[1].get(header::COOKIE).unwrap(),
            "sap-contextid=context-1"
        );
    }
}
