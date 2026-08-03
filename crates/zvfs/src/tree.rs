//! Lazy repository-tree storage and expansion.
//!
//! The graph is realized through a hash map, allowing constant look up
//! times to any nodes in the tree without having to traverse the path from
//! the root, as node references cannot cross network call boundaries.
//!
//! The tree graph is lazy, meaning nodes are only fetched at the time they are
//! needed. The definition of that time is left to the consumer of the vfs.
//!
//! Internally, each record wraps a public [`Node`] with private metadata and an
//! [`ExpansionStrategy`], which defines how that node will be expanded. Lock
//! retention is kept to an absolute minimum, locking the graph is only for read
//! and write access. During expansion (which inevitably invokes an I/O request),
//! a node-local mutex ensures data consistency on concurrent read requests without
//! keeping the rest of the graph locked.

mod expand;
mod graph;

use std::{collections::HashMap, sync::Arc};

use parking_lot::RwLock;
use zadt::{
    Client, Operation, Ready, RepositoryFacet, RepositoryFacetDefinition, RepositoryFacetsQuery,
    RepositoryObjectEntry,
};

use self::{
    expand::{ExpansionStrategy, PreparedNode},
    graph::Graph,
};
use crate::{Mount, Node, NodeId, NodeKind, VfsError};

type FacetCatalog = HashMap<RepositoryFacet, RepositoryFacetDefinition>;

/// A cheap, shared handle to a lazy repository tree.
///
/// Much like the ADT client it holds, it is safe to hold
/// references to this VFS in various places.
#[derive(Clone)]
pub struct VirtualRepositoryTree {
    inner: Arc<Inner>,
}

struct Inner {
    client: Client<Ready>,
    root: NodeId,
    graph: RwLock<Graph>,
    facets: FacetCatalog,
}

/// Configures and creates a [`VirtualRepositoryTree`].
pub struct VirtualRepositoryTreeBuilder {
    client: Client<Ready>,
    mounts: Vec<Mount>,
}

impl VirtualRepositoryTreeBuilder {
    fn new(client: Client<Ready>) -> Self {
        Self {
            client,
            mounts: Vec::new(),
        }
    }

    /// Adds one root mount.
    pub fn mount(mut self, mount: Mount) -> Self {
        self.mounts.push(mount);
        self
    }

    /// Adds multiple root mounts in iteration order.
    pub fn mounts(mut self, mounts: impl IntoIterator<Item = Mount>) -> Self {
        self.mounts.extend(mounts);
        self
    }

    /// Loads RIS facet capabilities, validates the mount policies, and builds the tree.
    ///
    /// Returns an error when capability discovery fails or a configured policy facet is
    /// unavailable for structuring repository results.
    pub async fn build(self) -> Result<VirtualRepositoryTree, VfsError> {
        let response = RepositoryFacetsQuery.execute(&self.client).await?;
        let facets = response
            .facets
            .into_iter()
            .map(|definition| (definition.facet(), definition))
            .collect::<FacetCatalog>();

        for mount in &self.mounts {
            for level in mount.facet_policy.levels() {
                let facet = level.facet();
                let definition = facets
                    .get(facet)
                    .ok_or_else(|| VfsError::UnsupportedFacet(facet.clone()))?;
                if !definition.is_for_structuring {
                    return Err(VfsError::UnstructuredFacet(facet.clone()));
                }
            }
        }

        VirtualRepositoryTree::from_builder(self, facets)
    }
}

impl VirtualRepositoryTree {
    /// Starts configuring a repository tree backed by an already discovered ADT client.
    pub fn builder(client: Client<Ready>) -> VirtualRepositoryTreeBuilder {
        VirtualRepositoryTreeBuilder::new(client)
    }

    fn from_builder(
        builder: VirtualRepositoryTreeBuilder,
        facets: FacetCatalog,
    ) -> Result<Self, VfsError> {
        let VirtualRepositoryTreeBuilder { client, mounts } = builder;
        let mut graph = Graph::new();
        let root = graph.insert(
            None,
            PreparedNode {
                label: "/".to_owned(),
                kind: NodeKind::Root,
                expansion: ExpansionStrategy::Static,
                object: None,
            },
        );

        let mut children = Vec::with_capacity(mounts.len());
        for mount in mounts {
            let node = PreparedNode::from_mount(mount, &client)?;
            children.push(
                graph
                    .insert(Some(root), node)
                    .index_for(graph.scope)
                    .expect("a newly inserted node belongs to its graph"),
            );
        }
        let root_index = graph
            .index(root)
            .expect("the root remains present while constructing the graph");
        graph
            .nodes
            .get_mut(&root_index)
            .expect("the root remains present while constructing the graph")
            .children = Some(children);

        Ok(Self {
            inner: Arc::new(Inner {
                client,
                root,
                graph: RwLock::new(graph),
                facets,
            }),
        })
    }

    /// Returns the static root node identity.
    pub fn root(&self) -> NodeId {
        self.inner.root
    }

    /// Returns a snapshot of an already known node without loading it.
    pub fn node(&self, id: NodeId) -> Option<Node> {
        let graph = self.inner.graph.read();
        let index = graph.index(id)?;
        graph.nodes.get(&index).map(|record| record.node.clone())
    }

    /// Returns a root-to-node snapshot path without encoding it as a filesystem URI.
    pub fn path(&self, id: NodeId) -> Result<Vec<Node>, VfsError> {
        let graph = self.inner.graph.read();
        let mut path = Vec::new();
        let mut current = Some(id);

        while let Some(current_id) = current {
            let index = graph
                .index(current_id)
                .ok_or(VfsError::UnknownNode(current_id))?;
            let record = graph
                .nodes
                .get(&index)
                .ok_or(VfsError::UnknownNode(current_id))?;
            path.push(record.node.clone());
            current = record.node.parent;
        }

        path.reverse();
        Ok(path)
    }

    /// Returns the retained ADT entry for an object node.
    pub fn object_entry(&self, id: NodeId) -> Result<RepositoryObjectEntry, VfsError> {
        let graph = self.inner.graph.read();
        let record = graph.record(id).ok_or(VfsError::UnknownNode(id))?;
        record.object.clone().ok_or(VfsError::NotObject(id))
    }

    /// Returns loaded children without starting an ADT request.
    pub fn cached_children(&self, id: NodeId) -> Result<Option<Vec<Node>>, VfsError> {
        let graph = self.inner.graph.read();
        let record = graph.record(id).ok_or(VfsError::UnknownNode(id))?;
        record
            .children
            .as_deref()
            .map(|children| graph.node_snapshots(children))
            .transpose()
    }

    /// Renders the currently materialized nodes as a directory tree.
    ///
    /// This method performs no ADT requests. Nodes that have not been expanded
    /// are included, but no descendants are shown for them.
    ///
    /// ```text
    /// /
    /// └── Package
    ///     └── Object
    /// ```
    pub fn render_tree(&self) -> String {
        let graph = self.inner.graph.read();
        let root_index = graph
            .index(self.inner.root)
            .expect("the root remains present for the lifetime of the VFS");
        let root = graph
            .nodes
            .get(&root_index)
            .expect("the root remains present for the lifetime of the VFS");
        let mut rendered = root.node.label.clone();
        let mut pending = Vec::new();

        if let Some(children) = &root.children {
            for (position, child) in children.iter().copied().enumerate().rev() {
                pending.push((child, String::new(), position + 1 == children.len()));
            }
        }

        while let Some((index, prefix, is_last)) = pending.pop() {
            let record = graph
                .nodes
                .get(&index)
                .expect("child indices always reference existing records");
            rendered.push('\n');
            rendered.push_str(&prefix);
            rendered.push_str(if is_last { "└── " } else { "├── " });
            rendered.push_str(&record.node.label);

            if let Some(children) = &record.children {
                let child_prefix = format!("{prefix}{}", if is_last { "    " } else { "│   " });
                for (position, child) in children.iter().copied().enumerate().rev() {
                    pending.push((child, child_prefix.clone(), position + 1 == children.len()));
                }
            }
        }

        rendered
    }

    /// Loads and caches one nodes immediate children. The graph lock is only held
    /// for the duration of the read / write on the graph. To ensure consistency on
    /// the load operation itself, a node-local mutex is locked.
    pub async fn children(&self, id: NodeId) -> Result<Vec<Node>, VfsError> {
        let load = {
            let graph = self.inner.graph.read();
            let record = graph.record(id).ok_or(VfsError::UnknownNode(id))?;
            if let Some(children) = &record.children {
                return graph.node_snapshots(children);
            }
            if matches!(record.expansion, ExpansionStrategy::Leaf) {
                return Err(VfsError::NotDirectory(id));
            }
            record.load.clone()
        };
        let _load_guard = load.lock().await;

        let (expansion, generation) = {
            // Another task may have populated the cache while we waited for this
            // nodes load lock, so check again before issuing a backend request.
            let graph = self.inner.graph.read();
            let record = graph.record(id).ok_or(VfsError::StaleNode(id))?;
            if let Some(children) = &record.children {
                return graph.node_snapshots(children);
            }
            (record.expansion.clone(), record.generation)
        };

        if matches!(expansion, ExpansionStrategy::Static) {
            return Err(VfsError::NotRefreshable(id));
        }

        // Insert the children into the tree first so that we can get
        // references to have the parent node point to
        let prepared = self.load(expansion, false).await?;
        let mut graph = self.inner.graph.write();
        let children = graph.install_loaded_children(
            id,
            generation,
            prepared.nodes,
            prepared.object_count,
            prepared.has_children_of_same_facet,
        )?;
        graph.node_snapshots(&children)
    }

    /// Best-effort loads one layer beneath every unloaded child of `id`.
    ///
    /// The immediate children are loaded first when necessary. Their expansion
    /// requests are then grouped into batch waves, while leaves and children
    /// already present in the cache are skipped. Callers that do not want to
    /// await this optimization can schedule the returned future on their runtime.
    pub async fn preload_all_children(&self, id: NodeId) -> Result<(), VfsError> {
        let children = self.children(id).await?;
        self.preload_children(children.into_iter().map(|child| child.id).collect())
            .await;
        Ok(())
    }

    /// Reloads one directory and atomically replaces its cached descendants.
    ///
    /// Existing children remain visible while the ADT request is in flight. On
    /// success, semantically unchanged children retain their IDs and compatible
    /// descendant caches. Removed children and incompatible cached descendants
    /// become stale.
    ///
    /// A concurrent ancestor refresh can remove this node or update its expansion
    /// inputs. In either case, work started from the old generation is discarded
    /// with an error.
    pub async fn refresh(&self, id: NodeId) -> Result<Vec<Node>, VfsError> {
        let (load, revision) = {
            let graph = self.inner.graph.read();
            let record = graph.record(id).ok_or(VfsError::UnknownNode(id))?;
            match record.expansion {
                ExpansionStrategy::Static => return Err(VfsError::NotRefreshable(id)),
                ExpansionStrategy::Leaf => return Err(VfsError::NotDirectory(id)),
                _ => (record.load.clone(), record.refresh_revision),
            }
        };
        let _load_guard = load.lock().await;

        let (expansion, generation) = {
            let graph = self.inner.graph.read();
            let record = graph.record(id).ok_or(VfsError::StaleNode(id))?;
            if record.refresh_revision != revision
                && let Some(children) = &record.children
            {
                return graph.node_snapshots(children);
            }
            (record.expansion.clone(), record.generation)
        };
        let prepared = self.load(expansion, true).await?;
        let mut graph = self.inner.graph.write();
        let children = graph.reconcile_children(id, generation, prepared.nodes)?;

        let record = graph.mut_record(id).ok_or(VfsError::StaleNode(id))?;
        record.install_children(
            children.clone(),
            prepared.object_count,
            prepared.has_children_of_same_facet,
        );
        record.advance_refresh_revision();
        graph.node_snapshots(&children)
    }
}
