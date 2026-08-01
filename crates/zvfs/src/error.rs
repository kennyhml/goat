use thiserror::Error;
use zadt::{OperationError, RepositoryContentQueryBuilderError};

use crate::NodeId;

/// An error produced while navigating or loading the virtual repository tree.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VfsError {
    #[error(transparent)]
    Operation(#[from] OperationError),

    #[error(transparent)]
    QueryBuilder(#[from] RepositoryContentQueryBuilderError),

    #[error("unknown VFS node {0:?}")]
    UnknownNode(NodeId),

    #[error("VFS node {0:?} became stale while it was loading")]
    StaleNode(NodeId),

    #[error("VFS node {0:?} is not a directory")]
    NotDirectory(NodeId),

    #[error("VFS node {0:?} is not a repository object")]
    NotObject(NodeId),

    #[error("VFS node {0:?} has static children and cannot be refreshed")]
    NotRefreshable(NodeId),
}
