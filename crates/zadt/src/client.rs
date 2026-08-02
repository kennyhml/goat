use std::sync::Arc;

use crate::{
    BatchOperation, Capabilities, CategoryId, Collection, CompatibilityError, Stateless, Transport,
    UserSession,
};

mod private {
    pub trait Sealed {}
}

/// Marker for the protocol lifecycle state of an ADT client.
///
/// ADT advertises top-level collections, URI templates, and accepted media
/// types through `/sap/bc/adt/discovery`. Operations that require those
/// capabilities implement `Operation<Ready>`, preventing their execution by a
/// client that has not completed discovery.
pub trait ClientState: private::Sealed + Clone + Send + Sync {}

/// The client has not loaded the server's central ADT capabilities.
#[derive(Clone, Debug, Default)]
pub struct Initial;

/// The client has loaded the server's central and core ADT capabilities.
///
/// This state records only a local capability snapshot. It does not guarantee
/// that authentication or the underlying transport remains alive.
#[derive(Clone, Debug)]
pub struct Ready {
    capabilities: Arc<Capabilities>,
    core_capabilities: Arc<Capabilities>,
}

impl private::Sealed for Initial {}
impl private::Sealed for Ready {}
impl ClientState for Initial {}
impl ClientState for Ready {}

/// A client for executing typed ADT operations.
///
/// The operations available to a client depend on the [`ClientState`] marker
/// `S`. The client owns protocol-level state such as loaded capabilities,
/// while request delivery is delegated to a [`Transport`] implementation.
///
/// A transport may send ADT requests over HTTP directly or through an adapter,
/// such as a future RFC bridge, without changing the operation API.
///
/// Because clients may be shared across different contexts, it must be possible
/// to clone it cheaply.
#[derive(Clone)]
pub struct Client<S = Initial> {
    transport: Arc<dyn Transport>,
    state: S,
}

impl Client<Initial> {
    /// Creates a client that has not loaded central ADT capabilities.
    pub fn new(transport: impl Transport + 'static) -> Self {
        Self {
            transport: Arc::new(transport),
            state: Initial,
        }
    }

    pub(crate) fn with_capabilities(
        self,
        capabilities: Capabilities,
        core_capabilities: Capabilities,
    ) -> Client<Ready> {
        Client {
            transport: self.transport,
            state: Ready {
                capabilities: Arc::new(capabilities),
                core_capabilities: Arc::new(core_capabilities),
            },
        }
    }
}

impl Client<Ready> {
    /// Creates an empty stateless batch using this client's core capabilities.
    pub fn batch(&self) -> Result<BatchOperation<Stateless>, CompatibilityError> {
        BatchOperation::new(self)
    }

    /// Returns the capabilities advertised by ADT.
    pub fn capabilities(&self) -> &Capabilities {
        &self.state.capabilities
    }

    /// Returns the infrastructure capabilities advertised by core discovery.
    pub fn core_capabilities(&self) -> &Capabilities {
        &self.state.core_capabilities
    }

    /// Returns the collection advertised for a category identity.
    pub fn collection(&self, category: CategoryId) -> Option<&Collection> {
        self.capabilities()
            .collection(category.scheme, category.term)
    }

    pub(crate) fn require_collection(
        &self,
        category: CategoryId,
    ) -> Result<&Collection, CompatibilityError> {
        self.collection(category)
            .ok_or(CompatibilityError::MissingCollection(category))
    }

    pub(crate) fn require_core_collection(
        &self,
        category: CategoryId,
    ) -> Result<&Collection, CompatibilityError> {
        self.core_capabilities()
            .collection(category.scheme, category.term)
            .ok_or(CompatibilityError::MissingCollection(category))
    }
}

impl<S: ClientState> Client<S> {
    pub(crate) fn transport(&self) -> &dyn Transport {
        self.transport.as_ref()
    }
    /// Creates an owned, long-lived SAP user session for stateful operations.
    ///
    /// The session is represented by `sap-contextid` over HTTP and can be
    /// inspected in transaction `SM04` while active.
    pub fn create_user_session(&self) -> UserSession<S> {
        UserSession::new(self.clone())
    }
}
