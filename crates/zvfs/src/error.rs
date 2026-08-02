use thiserror::Error;
use zadt::{ObjectError, OperationError, RepositoryContentQueryBuilderError, RepositoryFacet};

use crate::NodeId;

/// An error produced while navigating or loading the virtual repository tree.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VfsError {
    #[error(transparent)]
    Operation(#[from] OperationError),

    #[error(transparent)]
    QueryBuilder(#[from] RepositoryContentQueryBuilderError),

    #[error(transparent)]
    Object(#[from] ObjectError),

    #[error("repository facet `{0}` is not advertised by RIS")]
    UnsupportedFacet(RepositoryFacet),

    #[error("repository facet `{0}` cannot structure RIS results")]
    UnstructuredFacet(RepositoryFacet),

    #[error("repository package folder `{0}` did not advertise a resource URI")]
    MissingPackageUri(String),

    #[error(
        "repository response contains duplicate child identity `{identity}` below VFS node {parent:?}"
    )]
    DuplicateChildIdentity { parent: NodeId, identity: String },

    #[error("unknown VFS node {0:?}")]
    UnknownNode(NodeId),

    #[error("VFS node {0:?} changed or became stale while it was loading")]
    StaleNode(NodeId),

    #[error("VFS node {0:?} is not a directory")]
    NotDirectory(NodeId),

    #[error("VFS node {0:?} is not a repository object")]
    NotObject(NodeId),

    #[error("VFS node {0:?} has static children and cannot be refreshed")]
    NotRefreshable(NodeId),
}
