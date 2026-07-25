#![doc = include_str!("../README.md")]

mod client;
mod discovery;
mod error;
mod operation;
mod protocol;
mod uri;

pub mod transport;

pub use client::{Client, ClientState, Discovered, Undiscovered};
pub use discovery::{
    Capabilities, Category, Collection, CoreDiscoveryQuery, DiscoveryQuery, TemplateLink, Workspace,
};
#[cfg(feature = "reqwest")]
pub use error::ReqwestTransportBuildError;
pub use error::{DiscoveryError, OperationError, ResponseError, TransportError};
pub use operation::{Executor, Operation, OperationKind, Stateful, Stateless, UserSession};
pub use protocol::{AdtRequest, AdtResponse};
pub use transport::Transport;
#[cfg(feature = "reqwest")]
pub use transport::{ReqwestTransport, ReqwestTransportBuilder};
pub use uri::{ADT_RESOURCE_ROOT, ADT_ROOT, AdtUri, AdtUriError};
