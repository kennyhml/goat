use crate::{AdtRequest, AdtResponse, Client, ClientState, OperationError, ResponseError};
use std::future::Future;

mod private {
    pub trait Sealed {}
}

/// Identifies whether an ADT operation is [`Stateless`] or [`Stateful`].
///
/// Stateless operations do not require a persistent ADT user context. They may
/// still use authentication and an HTTP security session.
///
/// Stateful operations execute within a user context retained across requests.
/// For example, updating a program requires a lock acquired and used within the
/// same context. The context keeps the lock alive until it is released, closed,
/// or expires.
///
/// Requiring a context is part of the ADT operation contract. How that context
/// is represented on the wire, such as an HTTP `sap-contextid` cookie, depends
/// on the execution context and transport.
pub trait OperationKind: private::Sealed + Send + Sync {}

/// An operation that does not require a persistent ADT user context.
#[derive(Debug)]
pub struct Stateless;

/// An operation that requires a persistent ADT user context.
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
/// protocol contract from execution concerns such as user-context affinity,
/// session headers, serialization, and transport access.
///
/// The generic parameters express two independent requirements:
///
/// - `S` is the discovery state needed to build the operations request.
/// - `O` is the concrete operation and determines response and [`OperationKind`].
///
/// [`Client<S>`](Client) implements this trait only for [`Stateless`]
/// operations. Consequently, a [`Stateful`] operation cannot execute directly
/// through a client. A stateful execution context can implement this trait
/// while retaining the required ADT user context and delegating request
/// delivery to its client.
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
