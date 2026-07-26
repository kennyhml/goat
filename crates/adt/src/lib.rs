#![doc = include_str!("../README.md")]

mod api;
mod client;
mod compatibility;
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
    IncludeMediaVersion, IncludePropertiesQuery, IncludePropertiesRepresentation,
    ProgramMediaVersion, ProgramPropertiesQuery, ProgramPropertiesRepresentation, ProgramRun,
    ProgramRunBuilder, ProgramRunBuilderError,
};
pub use api::session::{Logon, SessionMediaVersion};
pub use client::{Client, ClientState, Discovered, LoggedOn, LoggedOnState, Unauthenticated};
pub use compatibility::{CompatibilityError, NegotiableMediaVersion, negotiate};
#[cfg(feature = "reqwest")]
pub use error::ReqwestTransportBuildError;
pub use error::{
    DiscoveryError, IncludeError, LogonError, ObjectError, OperationError, ProgramError,
    ResponseError, TransportError,
};
pub use models::{
    AccessMode, Capabilities, Category, Collection, IncludeProperties, LockHandle,
    ProgramProperties, ProgramRunOutput, SessionInformation, SessionUri, SourceCode,
    SyntaxConfiguration, SyntaxLanguage, SystemInformationLink, TemplateLink, Workspace,
};
pub use operation::{
    Conditional, Executor, IfNoneMatch, Operation, OperationKind, QueryMode, Stateful, Stateless,
    Unconditional, UserSession,
};
pub use protocol::{AdtRequest, AdtResponse, EntityTag};
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
