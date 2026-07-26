#![doc = include_str!("../README.md")]

mod api;
mod client;
mod discovery;
mod error;
mod object;
mod operation;
mod program;
mod protocol;
mod resource;
mod uri;
mod vocabulary;

pub mod transport;

pub use api::discovery::{CoreDiscoveryQuery, DiscoveryQuery};
pub use api::object::{
    ObjectLock, ObjectSourceQuery, ObjectSourceUpdate, ObjectSourceUpdateBuilder,
    ObjectSourceUpdateBuilderError, ObjectUnlock,
};
pub use api::program::{
    ProgramMediaVersion, ProgramQuery, ProgramQueryBuilder, ProgramQueryBuilderError,
    ProgramResponse,
};
pub use client::{Client, ClientState, Discovered, Undiscovered};
pub use discovery::{Capabilities, Category, Collection, TemplateLink, Workspace};
#[cfg(feature = "reqwest")]
pub use error::ReqwestTransportBuildError;
pub use error::{
    DiscoveryError, ObjectError, OperationError, ProgramError, ResponseError, TransportError,
};
pub use object::{AccessMode, LockHandle, SourceCode};
pub use operation::{Executor, Operation, OperationKind, Stateful, Stateless, UserSession};
pub use program::{Program, SyntaxConfiguration, SyntaxLanguage};
pub use protocol::{AdtRequest, AdtResponse};
pub use resource::{
    AdtLink, EnhancementImplementationsRef, EnhancementOptionsRef, FromDiscovery, HtmlSourceRef,
    ObjectRef, ObjectStateRef, ObjectStructureRef, ObjectVersion, PackageRef, ParserRef,
    ProgramRef, SourceRef, SourceVersionsRef, TextElementsRef,
};
pub use transport::Transport;
#[cfg(feature = "reqwest")]
pub use transport::{ReqwestTransport, ReqwestTransportBuilder};
pub use uri::{ADT_RESOURCE_ROOT, ADT_ROOT, AdtUri, AdtUriError};
pub use vocabulary::{CategoryId, PostAction};
