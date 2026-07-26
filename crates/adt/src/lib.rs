#![doc = include_str!("../README.md")]

mod api;
mod client;
mod error;
mod models;
mod operation;
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
pub use api::programs::{
    IncludeMediaVersion, IncludeQuery, IncludeQueryBuilder, IncludeQueryBuilderError,
    IncludeResponse, ProgramMediaVersion, ProgramQuery, ProgramQueryBuilder,
    ProgramQueryBuilderError, ProgramResponse,
};
pub use client::{Client, ClientState, Discovered, Undiscovered};
#[cfg(feature = "reqwest")]
pub use error::ReqwestTransportBuildError;
pub use error::{
    DiscoveryError, IncludeError, ObjectError, OperationError, ProgramError, ResponseError,
    TransportError,
};
pub use models::{
    AccessMode, Capabilities, Category, Collection, Include, LockHandle, Program, SourceCode,
    SyntaxConfiguration, SyntaxLanguage, TemplateLink, Workspace,
};
pub use operation::{Executor, Operation, OperationKind, Stateful, Stateless, UserSession};
pub use protocol::{AdtRequest, AdtResponse};
pub use resource::{
    AdtLink, EnhancementImplementationsRef, EnhancementOptionsRef, FromDiscovery, HtmlSourceRef,
    IncludeRef, ObjectRef, ObjectStateRef, ObjectStructureRef, ObjectVersion, PackageRef,
    ParserRef, ProgramRef, SourceRef, SourceVersionsRef, TextElementsRef,
};
pub use transport::Transport;
#[cfg(feature = "reqwest")]
pub use transport::{ReqwestTransport, ReqwestTransportBuilder};
pub use uri::{ADT_RESOURCE_ROOT, ADT_ROOT, AdtUri, AdtUriError};
pub use vocabulary::{CategoryId, PostAction};
