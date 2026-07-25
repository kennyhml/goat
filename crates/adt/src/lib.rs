#![doc = include_str!("../README.md")]

mod client;
mod discovery;
mod error;
mod object;
mod operation;
mod protocol;
mod resource;
mod uri;

pub mod transport;

pub use client::{Client, ClientState, Discovered, Undiscovered};
pub use discovery::{
    Capabilities, Category, Collection, CoreDiscoveryQuery, DiscoveryQuery, TemplateLink, Workspace,
};
#[cfg(feature = "reqwest")]
pub use error::ReqwestTransportBuildError;
pub use error::{
    DiscoveryError, ObjectError, OperationError, ProgramError, ResponseError, TransportError,
};
pub use object::{
    AccessMode, LockHandle, ObjectLock, ObjectSourceQuery, ObjectSourceUpdate,
    ObjectSourceUpdateBuilder, ObjectSourceUpdateBuilderError, ObjectUnlock, SourceCode,
};
pub use operation::{Executor, Operation, OperationKind, Stateful, Stateless, UserSession};
pub use protocol::{AdtRequest, AdtResponse};
pub use resource::{ObjectRef, ProgramRef, SourceRef};
pub use transport::Transport;
#[cfg(feature = "reqwest")]
pub use transport::{ReqwestTransport, ReqwestTransportBuilder};
pub use uri::{ADT_RESOURCE_ROOT, ADT_ROOT, AdtUri, AdtUriError};
