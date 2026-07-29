use std::sync::Arc;

use crate::{Capabilities, CategoryId, Collection, SessionInformation, Transport, UserSession};

mod private {
    pub trait Sealed {}
}

/// Marker for the protocol lifecycle state of an ADT client.
///
/// ADT advertises top-level collections, URI templates, and accepted media
/// types through `/sap/bc/adt/discovery`. Operations that require those
/// capabilities implement `Operation<Discovered>`, preventing their execution
/// by a client that has not completed discovery. Other authenticated operations
/// accept any [`LoggedOnState`].
pub trait ClientState: private::Sealed + Clone + Send + Sync {}

/// A client state carrying an authenticated HTTP security session.
pub trait LoggedOnState: ClientState {
    #[doc(hidden)]
    fn session_information(&self) -> &SessionInformation;
}

/// The client has not established an authenticated ADT session.
#[derive(Clone, Debug, Default)]
pub struct Unauthenticated;

/// The client has established an authenticated ADT session.
#[derive(Clone, Debug)]
pub struct LoggedOn {
    session_information: Arc<SessionInformation>,
}

/// The client is logged on and has fetched the server's central capabilities.
#[derive(Clone, Debug)]
pub struct Discovered {
    session_information: Arc<SessionInformation>,
    capabilities: Arc<Capabilities>,
}

impl private::Sealed for Unauthenticated {}
impl private::Sealed for LoggedOn {}
impl private::Sealed for Discovered {}
impl ClientState for Unauthenticated {}
impl ClientState for LoggedOn {}
impl ClientState for Discovered {}

impl LoggedOnState for LoggedOn {
    fn session_information(&self) -> &SessionInformation {
        &self.session_information
    }
}

impl LoggedOnState for Discovered {
    fn session_information(&self) -> &SessionInformation {
        &self.session_information
    }
}

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
pub struct Client<S = Unauthenticated> {
    transport: Arc<dyn Transport>,
    state: S,
}

impl Client<Unauthenticated> {
    /// Creates an unauthenticated client using the supplied transport.
    pub fn new(transport: impl Transport + 'static) -> Self {
        Self {
            transport: Arc::new(transport),
            state: Unauthenticated,
        }
    }

    pub(crate) fn with_session_information(
        self,
        session_information: SessionInformation,
    ) -> Client<LoggedOn> {
        Client {
            transport: self.transport,
            state: LoggedOn {
                session_information: Arc::new(session_information),
            },
        }
    }
}

impl Client<LoggedOn> {
    pub(crate) fn with_capabilities(self, capabilities: Capabilities) -> Client<Discovered> {
        Client {
            transport: self.transport,
            state: Discovered {
                session_information: self.state.session_information,
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

    /// Returns the collection advertised for a category identity.
    pub fn collection(&self, category: CategoryId) -> Option<&Collection> {
        self.capabilities()
            .collection(category.scheme, category.term)
    }
}

impl<S: ClientState> Client<S> {
    pub(crate) fn transport(&self) -> &dyn Transport {
        self.transport.as_ref()
    }
}

impl<S: LoggedOnState> Client<S> {
    /// Returns information about the authenticated HTTP security session.
    pub fn session_information(&self) -> &SessionInformation {
        self.state.session_information()
    }

    /// Creates an owned, long-lived SAP user session for stateful operations.
    ///
    /// The session is represented by `sap-contextid` over HTTP and can be
    /// inspected in transaction `SM04` while active.
    pub fn create_user_session(&self) -> UserSession<S> {
        UserSession::new(self.clone())
    }
}
