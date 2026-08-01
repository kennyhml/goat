#![doc = include_str!("../README.md")]

mod config;
mod error;
mod node;
mod tree;

pub use config::{FacetLevel, FacetPolicy, Mount};
pub use error::VfsError;
pub use node::{MountKind, Node, NodeId, NodeKind, ObjectNode};
pub use tree::{VirtualRepositoryTree, VirtualRepositoryTreeBuilder};
