use std::sync::Arc;

use crate::{Capabilities, FromDiscovery, Transport, UserSession};

mod private {
    pub trait Sealed {}
}

/// Marker for the central-discovery state of an ADT client.
///
/// ADT advertises top-level collections, URI templates, and accepted media
/// types through `/sap/bc/adt/discovery`. Operations that require those
/// capabilities implement `Operation<Discovered>`, preventing their execution
/// by an undiscovered client.
///
/// Fixed bootstrap operations such as `CoreDiscoveryQuery` and
/// `DiscoveryQuery` can execute in either client state because their locations
/// are known in advance.
///
/// This state only indicates that central discovery has been loaded. It does
/// not imply that every resource location is known; related resources may
/// still need to be resolved from links in resource representations.
pub trait ClientState: private::Sealed + Clone + Send + Sync {}

/// The client has not fetched the server's ADT discovery document yet.
#[derive(Clone, Debug, Default)]
pub struct Undiscovered;

/// The client has fetched and validated the server's ADT capabilities.
#[derive(Clone, Debug)]
pub struct Discovered {
    capabilities: Arc<Capabilities>,
}

impl private::Sealed for Undiscovered {}
impl private::Sealed for Discovered {}
impl ClientState for Undiscovered {}
impl ClientState for Discovered {}

/// A client for executing typed ADT operations.
///
/// The operations available to a client depend on the [`ClientState`] marker
/// `S`. The client owns protocol-level state such as discovered capabilities,
/// while request delivery is delegated to a [`Transport`] implementation.
///
/// A transport may send ADT requests over HTTP directly or through an adapter,
/// such as a future RFC bridge, without changing the operation API.
///
/// Because clients may be shared across different contexts, it must be possible
/// to clone it cheaply.
#[derive(Clone)]
pub struct Client<S = Undiscovered> {
    transport: Arc<dyn Transport>,
    state: S,
}

impl Client<Undiscovered> {
    /// Creates an undiscovered client using the supplied transport.
    pub fn new(transport: impl Transport + 'static) -> Self {
        Self {
            transport: Arc::new(transport),
            state: Undiscovered,
        }
    }

    pub(crate) fn with_capabilities(self, capabilities: Capabilities) -> Client<Discovered> {
        Client {
            transport: self.transport,
            state: Discovered {
                capabilities: Arc::new(capabilities),
            },
        }
    }
}

impl Client<Discovered> {
    /// Returns the capabilities advertised by ADT.
    pub fn capabilities(&self) -> &Capabilities {
        &self.state.capabilities
    }

    /// Resolves a typed object reference from central discovery.
    ///
    /// `T` identifies the collection category and interprets that collections
    /// member convention. Resolving a reference performs does not perform I/O,
    /// it is simply resolved against the dicovered collections
    ///
    /// ```rust,ignore
    /// use goat_adt::ProgramRef;
    ///
    /// let program = client.object::<ProgramRef>("ZDEMO")?;
    /// ```
    pub fn object<T>(&self, name: &str) -> Result<T, T::Error>
    where
        T: FromDiscovery,
    {
        T::from_discovery(self.capabilities(), name)
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
