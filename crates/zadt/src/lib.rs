#![doc = include_str!("../README.md")]

mod api;
mod client;
mod compatibility;
mod error;
mod models;
mod objects;
mod operation;
mod protocol;
mod resource;
mod target;
mod uri;
mod vocabulary;

pub mod transport;

pub use api::discovery::{CoreDiscoveryQuery, DiscoveryQuery};
pub use api::object::{
    LockRequest, ObjectSourceQuery, ObjectSourceUpdate, ObjectSourceUpdateBuilder,
    ObjectSourceUpdateBuilderError, UnlockRequest,
};
pub use api::programs::{
    IncludePropertiesQuery, ProgramPropertiesQuery, ProgramRun, ProgramRunBuilder,
    ProgramRunBuilderError,
};
pub use api::properties::ObjectPropertiesQuery;
pub use api::repository::{
    RepositoryContentOperation, RepositoryContentQuery, RepositoryContentQueryBuilder,
    RepositoryContentQueryBuilderError, RepositoryFacetsQuery, RepositoryObjectPropertiesQuery,
    RepositoryObjectPropertiesQueryBuilder, RepositoryObjectPropertiesQueryBuilderError,
};
pub use api::session::{Logon, SessionMediaVersion};
pub use client::{Client, ClientState, Initial, Ready};
pub use compatibility::{CompatibilityError, NegotiableMediaVersion, negotiate};
#[cfg(feature = "reqwest")]
pub use error::ReqwestTransportBuildError;
pub use error::{
    DiscoveryError, LogonError, ObjectError, OperationError, RepositoryError, ResponseError,
    TransportError,
};
pub use models::{
    AccessMode, Capabilities, Category, Collection, IncludeProperties, IncludePropertiesV2,
    IncludePropertyVersion, LockHandle, ProgramProperties, ProgramPropertiesV2,
    ProgramPropertiesV3, ProgramPropertiesVersion, ProgramRunResult, RepositoryContent,
    RepositoryFacet, RepositoryFacetDefinition, RepositoryFacetValuesLink, RepositoryFacets,
    RepositoryObjectEntry, RepositoryObjectProperties, RepositoryObjectSummary,
    RepositoryObjectType, RepositoryPreselection, RepositoryPreselectionInfo, RepositoryProperty,
    RepositoryVirtualFolder, SessionInformation, SessionUri, SourceCode, SyntaxConfiguration,
    SyntaxLanguage, SystemInformationLink, TemplateLink, Workspace,
};
pub use objects::{
    GlobalWorkbenchType, Include, InvalidWorkbenchType, ObjectCollection, ObjectNamePolicy,
    ObjectProperties, ObjectRef, ObjectType, ObjectVersion, Package, Program, RepositoryObject,
    Source,
};
pub use operation::{
    Conditional, Executor, IfNoneMatch, Operation, OperationKind, QueryMode, Stateful, Stateless,
    Unconditional, UserSession,
};
pub use protocol::{AdtRequest, AdtResponse, EntityTag};
pub use resource::{
    AdtLink, AdtLinkError, EnhancementImplementationsRef, HtmlSourceRef,
    ObjectEnhancementOptionsRef, ObjectStateRef, ObjectStructureRef, OwnedResourceRef, ParserRef,
    Relations, SourceEnhancementOptionsRef, SourceRef, SourceVersionsRef, TextElementsRef,
};
pub use transport::Transport;
#[cfg(feature = "reqwest")]
pub use transport::{ReqwestTransport, ReqwestTransportBuilder};
#[cfg(feature = "logging")]
pub use transport::{Traced, TransportExt};
pub use uri::{ADT_RESOURCE_ROOT, ADT_ROOT, AdtUri, AdtUriError};
pub use vocabulary::{CategoryId, PostAction};
