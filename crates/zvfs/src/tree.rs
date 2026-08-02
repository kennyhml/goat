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
use std::{cmp::Ordering, collections::HashMap, sync::Arc};

use async_lock::Mutex;
use futures_util::future::try_join;
use parking_lot::RwLock;
use uuid::Uuid;
use zadt::{
    Client, Operation, Package, Ready, RepositoryContent, RepositoryContentOperation,
    RepositoryContentQuery, RepositoryFacet, RepositoryFacetDefinition, RepositoryFacetsQuery,
    RepositoryObjectEntry, RepositoryPreselection, RepositoryVirtualFolder,
};

use crate::{
    FacetPolicy, Mount, MountKind, Node, NodeId, NodeKind, ObjectNode, VfsError,
    config::MountTarget,
};

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

/// Mutable node storage for one virtual repository tree.
///
/// Access must be synchronized because lazy expansion and refresh mutate the
/// graph through shared handles.
///
/// Public node IDs contain a graph scope and numeric index. Each externally
/// supplied ID is validated against this graph's scope before its index is used.
/// Internally, record-map keys and child links use only numeric indices.
///
/// Indices are never reused, so a stale ID cannot resolve to a node inserted
/// after the original record was removed.
struct Graph {
    scope: Uuid,
    next_index: u64,
    nodes: HashMap<u64, NodeRecord>,
}

impl Graph {
    fn new() -> Self {
        Self {
            scope: Uuid::new_v4(),
            next_index: 0,
            nodes: HashMap::new(),
        }
    }

    /// Resolves a scoped node ID to its live internal index.
    ///
    /// IDs from another tree and IDs whose records have been removed are both
    /// rejected.
    fn index(&self, id: NodeId) -> Option<u64> {
        id.index_for(self.scope)
            .filter(|index| self.nodes.contains_key(index))
    }

    /// Returns the live record for a node ID belonging to this tree.
    fn record(&self, id: NodeId) -> Option<&NodeRecord> {
        let index = id.index_for(self.scope)?;
        self.nodes.get(&index)
    }

    /// Returns the live record mutably for a node ID belonging to this tree.
    fn mut_record(&mut self, id: NodeId) -> Option<&mut NodeRecord> {
        let index = id.index_for(self.scope)?;
        self.nodes.get_mut(&index)
    }

    /// Inserts one prepared node and assigns it the next scoped identity.
    ///
    /// Indices are consumed monotonically and remain reserved after removal.
    /// This method does not validate `parent`. Callers inserting children from
    /// asynchronously loaded data must use [`Graph::try_insert_children`].
    fn insert(&mut self, parent: Option<NodeId>, node: PreparedNode) -> NodeId {
        let index = self.next_index;
        self.next_index = self
            .next_index
            .checked_add(1)
            .expect("a VFS cannot exhaust all u64 node identities");

        let id = NodeId::new(self.scope, index);
        self.nodes.insert(
            index,
            NodeRecord::new(
                Node {
                    id,
                    parent,
                    label: node.label,
                    kind: node.kind,
                },
                node.expansion,
                node.object,
            ),
        );
        id
    }

    /// Inserts prepared children if `parent` is still present in this graph.
    ///
    /// Parent validation happens before any identities are allocated. This
    /// prevents an ancestor refresh that completed while backend work was in
    /// flight from leaving newly inserted children without a live parent.
    ///
    /// Returns the childrens internal indices in iteration order, or `None`
    /// without inserting anything when the parent is absent.
    fn try_insert_children(
        &mut self,
        parent: NodeId,
        children: impl IntoIterator<Item = PreparedNode>,
    ) -> Option<Vec<u64>> {
        self.record(parent)?;

        Some(
            children
                .into_iter()
                .map(|node| {
                    self.insert(Some(parent), node)
                        .index_for(self.scope)
                        .expect("a newly inserted node belongs to its graph")
                })
                .collect(),
        )
    }

    /// Returns owned snapshots for the given internal node indices.
    ///
    /// Nodes are cloned so the returned values do not borrow the graph or extend
    /// the lifetime of its lock guard. Input order is preserved, and later graph
    /// updates do not affect the snapshots.
    fn node_snapshots(&self, ids: &[u64]) -> Result<Vec<Node>, VfsError> {
        ids.iter()
            .map(|id| {
                self.nodes
                    .get(id)
                    .map(|record| record.node.clone())
                    .ok_or(VfsError::UnknownNode(NodeId::new(self.scope, *id)))
            })
            .collect()
    }

    /// Removes each supplied root and all of its materialized descendants.
    ///
    /// Missing roots are ignored. Removed indices remain reserved and are not
    /// assigned to future nodes.
    fn remove_subtrees(&mut self, roots: Vec<u64>) {
        let mut pending = roots;
        while let Some(id) = pending.pop() {
            if let Some(record) = self.nodes.remove(&id)
                && let Some(children) = record.children
            {
                pending.extend(children);
            }
        }
    }
}

/// Internal state retained for a public node snapshot.
///
/// `children = None` means the node has not been expanded. The async load lock
/// serializes requests for this node without blocking expansion of other nodes.
struct NodeRecord {
    node: Node,
    expansion: ExpansionStrategy,
    object: Option<RepositoryObjectEntry>,
    children: Option<Vec<u64>>,
    load: Arc<Mutex<()>>,
}

impl NodeRecord {
    fn new(
        node: Node,
        expansion: ExpansionStrategy,
        object: Option<RepositoryObjectEntry>,
    ) -> Self {
        Self {
            node,
            expansion,
            object,
            children: None,
            load: Arc::new(Mutex::new(())),
        }
    }

    /// Applies newly loaded facet metadata to both the public node representation
    /// and its internal expansion state. `None` leaves the existing value unchanged.
    fn update_facet_state(
        &mut self,
        object_count: Option<u32>,
        has_children_of_same_facet: Option<bool>,
    ) {
        if let NodeKind::Facet {
            object_count: count,
            has_children_of_same_facet: has_children,
            ..
        } = &mut self.node.kind
        {
            if let Some(object_count) = object_count {
                *count = object_count;
            }
            if let Some(has_children_of_same_facet) = has_children_of_same_facet {
                *has_children = has_children_of_same_facet;
            }
        }
        if let ExpansionStrategy::Facet {
            object_count: count,
            has_children_of_same_facet: has_children,
            ..
        } = &mut self.expansion
        {
            if let Some(object_count) = object_count {
                *count = object_count;
            }
            if let Some(has_children_of_same_facet) = has_children_of_same_facet {
                *has_children = has_children_of_same_facet;
            }
        }
    }
}

/// Immutable mount configuration and the complete RIS filter path to one node.
///
/// The vector of preselections must be cloned per node.
#[derive(Clone)]
struct ExpansionContext {
    facet_policy: Arc<FacetPolicy>,
    preselections: Vec<RepositoryPreselection>,
}

impl ExpansionContext {
    fn new(facet_policy: FacetPolicy, preselections: Vec<RepositoryPreselection>) -> Self {
        Self {
            facet_policy: Arc::new(facet_policy),
            preselections,
        }
    }

    fn facet_policy(&self) -> &FacetPolicy {
        &self.facet_policy
    }

    fn preselections(&self) -> &[RepositoryPreselection] {
        &self.preselections
    }

    fn with(&self, preselection: RepositoryPreselection) -> Self {
        let mut context = self.clone();
        // RIS intersects repeated same-facet entries, while retaining them also
        // preserves the complete path through hierarchical facets.
        context.preselections.push(preselection);
        context
    }

    /// Creates a new [`ExpansionStrategy`] for this context where the given package
    /// is used to expand upon. The expansion policy is shared.
    ///
    /// We can tell this package expansion whether it has child packages at this time to
    /// preemptively prevent child package probing if that is not the case.
    fn child_package(&self, package: String, has_child_packages: bool) -> ExpansionStrategy {
        ExpansionStrategy::Package {
            package,
            context: self.clone(),
            has_child_packages,
        }
    }

    /// Creates a new [`ExpansionStrategy`] whose context includes the selected facet value.
    ///
    /// For example, selecting a group folder while browsing a package produces
    /// the following transition:
    ///
    /// ```text
    /// Parent query:
    ///   preselections: [PACKAGE=../ROOT]
    ///   output facet:  GROUP (index 0)
    ///
    /// Selected folder:
    ///   GROUP=SOURCE_LIBRARY
    ///
    /// Child expansion:
    ///   preselections: [PACKAGE=../ROOT, GROUP=SOURCE_LIBRARY]
    ///   facet index:   0
    ///
    /// Expanding that child:
    ///   same-facet children: GROUP (index 0)
    ///   otherwise:           TYPE  (index 1)
    /// ```
    ///
    /// This method stores the index of the facet that produced the child. The
    /// expansion logic later decides whether to repeat or advance that index.
    fn child_facet(
        &self,
        preselection: RepositoryPreselection,
        facet_index: usize,
        object_count: u32,
        has_children_of_same_facet: bool,
    ) -> ExpansionStrategy {
        ExpansionStrategy::Facet {
            context: self.with(preselection),
            facet_index,
            object_count,
            has_children_of_same_facet,
        }
    }
}

/// Describes how one directory obtains its immediate children.
#[derive(Clone)]
enum ExpansionStrategy {
    /// A directory whose children were installed while constructing the VFS.
    Static,
    /// The top-level package hierarchy used by a system-library mount.
    PackageIndex { context: ExpansionContext },
    /// One package, expanded into child packages and directly assigned content.
    Package {
        package: String,
        context: ExpansionContext,
        has_child_packages: bool,
    },
    /// An arbitrary caller-provided RIS selection.
    Selection { context: ExpansionContext },
    /// A virtual folder within the configured facet chain.
    Facet {
        context: ExpansionContext,
        facet_index: usize,
        object_count: u32,
        has_children_of_same_facet: bool,
    },
    /// A repository object, which cannot be expanded by this tree.
    Leaf,
}

/// A node prepared off-graph and assigned an identity when committed.
struct PreparedNode {
    label: String,
    kind: NodeKind,
    expansion: ExpansionStrategy,
    object: Option<RepositoryObjectEntry>,
}

impl PreparedNode {
    /// Constructs a prepared node from a virtual repository folder representing a package.
    ///
    /// This assumes that it has already been verified that the folder is a package.
    fn from_package_folder(
        folder: RepositoryVirtualFolder,
        ctx: &ExpansionContext,
    ) -> Result<Self, VfsError> {
        let uri = folder
            .uri
            .ok_or_else(|| VfsError::MissingPackageUri(folder.name.clone()))?;
        Ok(Self {
            label: folder.name.clone(),
            kind: NodeKind::Package {
                package: folder.name.clone(),
                uri,
                object_count: Some(folder.object_count),
            },
            expansion: ctx.child_package(folder.name, folder.has_children_of_same_facet),
            object: None,
        })
    }

    fn tree_order(&self, other: &Self) -> Ordering {
        self.kind_order(other).then_with(|| self.label_order(other))
    }

    fn kind_order(&self, other: &Self) -> Ordering {
        self.kind.rank().cmp(&other.kind.rank())
    }

    fn label_order(&self, other: &Self) -> Ordering {
        let left = self.label.to_ascii_lowercase();
        let right = other.label.to_ascii_lowercase();

        left.cmp(&right).then_with(|| self.label.cmp(&other.label))
    }
}

impl PreparedNode {
    fn from_mount(mount: Mount, client: &Client<Ready>) -> Result<Self, VfsError> {
        let Mount {
            label,
            target,
            facet_policy,
        } = mount;
        Ok(match target {
            MountTarget::SystemLibrary => Self {
                label,
                kind: NodeKind::Mount {
                    mount: MountKind::SystemLibrary,
                },
                // Load all packages
                expansion: ExpansionStrategy::PackageIndex {
                    context: ExpansionContext::new(facet_policy, Vec::new()),
                },
                object: None,
            },
            MountTarget::Package(package) => Self {
                label,
                kind: NodeKind::Package {
                    package: package.clone(),
                    uri: client.object::<Package>(&package)?.uri().clone(),
                    object_count: None,
                },
                // Load sub packages
                expansion: ExpansionStrategy::Package {
                    package,
                    context: ExpansionContext::new(facet_policy, Vec::new()),
                    // Explicit mounts have no folder metadata, so probe conservatively.
                    has_child_packages: true,
                },
                object: None,
            },
            MountTarget::Selection(preselections) => Self {
                label,
                kind: NodeKind::Mount {
                    mount: MountKind::Selection,
                },
                // Load custom preselection
                expansion: ExpansionStrategy::Selection {
                    context: ExpansionContext::new(facet_policy, preselections),
                },
                object: None,
            },
        })
    }
}

impl From<RepositoryObjectEntry> for PreparedNode {
    fn from(entry: RepositoryObjectEntry) -> Self {
        let object = ObjectNode {
            name: entry.name.clone(),
            package: entry.package.clone(),
            object_type: entry.object_type.to_string(),
            uri: entry.reference.uri().clone(),
            virtual_workbench_uri: entry.virtual_workbench_uri.clone(),
            version: entry.version.clone(),
            expandable: entry.expandable,
            description: entry.description.clone(),
        };
        PreparedNode {
            label: entry.name.clone(),
            kind: NodeKind::Object { object },
            expansion: ExpansionStrategy::Leaf,
            object: Some(entry),
        }
    }
}

struct Loaded {
    prepared: Vec<PreparedNode>,
    object_count: Option<u32>,
    has_children_of_same_facet: Option<bool>,
}

/// Represents a layer in the repository tree that has been loaded.
///
/// The node contents of this layer may not be homogeneous. For example, a
/// package expansion may return both child packages and directly assigned
/// development objects.
struct LoadedLayer {
    nodes: Vec<PreparedNode>,
    object_count: u32,
}

impl LoadedLayer {
    /// Converts a set of packages in the form of virtual folders from a [`RepositoryContent`]
    /// reply into their corresponding prepared nodes and wraps them in a layer of loaded objects.
    ///
    /// Crucially, package replies may include things that should not actually become part
    /// of the tree. For instance, the `../DMO/PACKAGE` notation for directly assigned objects.
    fn from_packages(content: RepositoryContent, ctx: ExpansionContext) -> Result<Self, VfsError> {
        let object_count = content.object_count;
        let mut nodes = content
            .folders
            .into_iter()
            .filter(|f| f.facet == RepositoryFacet::PACKAGE && !f.is_direct_assignment())
            .map(|f| PreparedNode::from_package_folder(f, &ctx))
            .collect::<Result<Vec<_>, _>>()?;

        nodes.sort_by(PreparedNode::tree_order);
        Ok(Self {
            nodes,
            object_count,
        })
    }

    /// Converts a set of virtual folders from a [`RepositoryContent`] reply into
    /// their corresponding prepared nodes and wraps them in a layer of loaded objects.
    fn from_folders(content: RepositoryContent, ctx: ExpansionContext, facet_index: usize) -> Self {
        let mut nodes = content
            .folders
            .into_iter()
            .map(|f| {
                // Add this folders facet/value to the current preselections.
                // `facet_index` identifies the policy level that produced the folder.
                let expansion = ctx.child_facet(
                    f.as_preselection(),
                    facet_index,
                    f.object_count,
                    f.has_children_of_same_facet,
                );
                PreparedNode {
                    label: f.name_or_technical_name().to_owned(),
                    kind: NodeKind::Facet {
                        facet: f.facet.to_string(),
                        value: f.name,
                        object_count: f.object_count,
                        has_children_of_same_facet: f.has_children_of_same_facet,
                    },
                    expansion,
                    object: None,
                }
            })
            .collect::<Vec<_>>();
        nodes.sort_by(PreparedNode::tree_order);

        Self {
            nodes,
            object_count: content.object_count,
        }
    }

    fn from_objects(content: RepositoryContent) -> Self {
        let object_count = content.object_count;
        let mut nodes = content
            .objects
            .into_iter()
            .map(From::from)
            .collect::<Vec<_>>();
        nodes.sort_by(PreparedNode::tree_order);

        Self {
            nodes,
            object_count,
        }
    }
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

        let expansion = {
            // Another task may have populated the cache while we waited for this
            // nodes load lock, so check again before issuing a backend request.
            let graph = self.inner.graph.read();
            let record = graph.record(id).ok_or(VfsError::StaleNode(id))?;
            if let Some(children) = &record.children {
                return graph.node_snapshots(children);
            }
            record.expansion.clone()
        };

        if matches!(expansion, ExpansionStrategy::Static) {
            return Err(VfsError::NotRefreshable(id));
        }

        // Insert the children into the tree first so that we can get
        // references to have the parent node point to
        let loaded = self.load(expansion, false).await?;
        let mut graph = self.inner.graph.write();
        let children = graph
            .try_insert_children(id, loaded.prepared)
            .ok_or(VfsError::StaleNode(id))?;

        // Update the parent with the references and the possibly new
        // repository metadata to keep it synced
        let record = graph.mut_record(id).ok_or(VfsError::StaleNode(id))?;
        record.update_facet_state(loaded.object_count, loaded.has_children_of_same_facet);
        record.children = Some(children.clone());
        graph.node_snapshots(&children)
    }

    /// Reloads one directory and atomically replaces its cached descendants.
    ///
    /// Existing children remain visible while the ADT request is in flight. On
    /// success, their IDs become stale and newly loaded children replace them.
    ///
    /// A concurrent task could simultaneously erase this node from the vfs, for
    /// example when an ancestor is refreshed. In that case, this node becomes stale
    /// and the result is discarded with an error.
    pub async fn refresh(&self, id: NodeId) -> Result<Vec<Node>, VfsError> {
        let load = {
            let graph = self.inner.graph.read();
            let record = graph.record(id).ok_or(VfsError::UnknownNode(id))?;
            match record.expansion {
                ExpansionStrategy::Static => return Err(VfsError::NotRefreshable(id)),
                ExpansionStrategy::Leaf => return Err(VfsError::NotDirectory(id)),
                _ => record.load.clone(),
            }
        };
        let _load_guard = load.lock().await;

        let expansion = {
            let graph = self.inner.graph.read();
            let record = graph.record(id).ok_or(VfsError::StaleNode(id))?;
            record.expansion.clone()
        };
        let loaded = self.load(expansion, true).await?;
        let mut graph = self.inner.graph.write();

        // Remove the current children recursively, they are now stale
        // TODO: Implement reconciliation instead of busting it all?
        if let Some(children) = graph
            .mut_record(id)
            .ok_or(VfsError::StaleNode(id))?
            .children
            .take()
        {
            graph.remove_subtrees(children);
        }

        let children = graph
            .try_insert_children(id, loaded.prepared)
            .ok_or(VfsError::StaleNode(id))?;

        let record = graph.mut_record(id).ok_or(VfsError::StaleNode(id))?;
        record.update_facet_state(loaded.object_count, loaded.has_children_of_same_facet);
        record.children = Some(children.clone());
        graph.node_snapshots(&children)
    }

    /// Executes an expansion strategy and returns children not yet inserted into the graph.
    async fn load(&self, expansion: ExpansionStrategy, refresh: bool) -> Result<Loaded, VfsError> {
        match expansion {
            ExpansionStrategy::Static => unreachable!("static nodes have preloaded children"),
            // Loading of all packages, likely from the system library
            ExpansionStrategy::PackageIndex { context } => {
                let content = self
                    .query_content(context.preselections(), Some(&RepositoryFacet::PACKAGE))
                    .await?;
                Ok(Loaded {
                    prepared: LoadedLayer::from_packages(content, context)?.nodes,
                    object_count: None,
                    has_children_of_same_facet: None,
                })
            }
            // Loading of the contents of a package, including directly assigned objects
            // and child packages if applicable.
            ExpansionStrategy::Package {
                package: pkg,
                context: ctx,
                has_child_packages: mut probe_child_packages,
            } => {
                probe_child_packages |= refresh;
                Ok(Loaded {
                    prepared: self.load_package(pkg, ctx, probe_child_packages).await?,
                    object_count: None,
                    has_children_of_same_facet: None,
                })
            }
            // Loading via a custom selection strategy
            ExpansionStrategy::Selection { context } => {
                let layer = self.load_next_content_layer(context, 0).await?;
                Ok(Loaded {
                    prepared: layer.nodes,
                    object_count: None,
                    has_children_of_same_facet: None,
                })
            }
            // Loading of a facet using its facet index to point to the current policy
            ExpansionStrategy::Facet {
                context,
                facet_index,
                has_children_of_same_facet,
                ..
            } => {
                if refresh {
                    return self.refresh_facet(context, facet_index).await;
                }
                // Hierarchical facets repeat until RIS marks the selected folder as a leaf.
                let next_facet = if has_children_of_same_facet {
                    facet_index
                } else {
                    facet_index + 1
                };
                let layer = self.load_next_content_layer(context, next_facet).await?;
                Ok(Loaded {
                    prepared: layer.nodes,
                    object_count: Some(layer.object_count),
                    has_children_of_same_facet: None,
                })
            }
            ExpansionStrategy::Leaf => unreachable!("leaf expansion is rejected before loading"),
        }
    }

    /// Re-probes a facet so changes to same-facet hierarchy are discovered.
    ///
    /// This needs special care as, depending on the facet, its possible that the
    /// `has_children_of_same_facet` on our side no longer matches the actual state.
    async fn refresh_facet(
        &self,
        ctx: ExpansionContext,
        facet_index: usize,
    ) -> Result<Loaded, VfsError> {
        let level = ctx
            .facet_policy()
            .levels()
            .get(facet_index)
            .expect("a facet expansion references its policy level");

        let definition = self
            .inner
            .facets
            .get(level.facet())
            .expect("facet policies are validated ahead of time");

        // Facets that are not hierarchical (most of them) can not have new
        // children of the same facet.
        if !definition.is_hierarchical {
            let layer = self.load_next_content_layer(ctx, facet_index + 1).await?;
            return Ok(Loaded {
                prepared: layer.nodes,
                object_count: Some(layer.object_count),
                has_children_of_same_facet: Some(false),
            });
        }

        // Query again to discover whether same-facet children were added or removed.
        let facet = level.facet().clone();
        let same_facet = self
            .query_content(ctx.preselections(), Some(&facet))
            .await?;

        if !same_facet.folders.is_empty() {
            let object_count = same_facet.object_count;
            let specs = if level.retains(object_count) {
                LoadedLayer::from_folders(same_facet, ctx, facet_index).nodes
            } else {
                self.load_next_content_layer(ctx, facet_index + 1)
                    .await?
                    .nodes
            };
            return Ok(Loaded {
                prepared: specs,
                object_count: Some(object_count),
                has_children_of_same_facet: Some(true),
            });
        }

        let layer = self.load_next_content_layer(ctx, facet_index + 1).await?;
        Ok(Loaded {
            prepared: layer.nodes,
            object_count: Some(layer.object_count),
            has_children_of_same_facet: Some(false),
        })
    }

    /// Loads directly assigned content and, when needed, child packages.
    ///
    /// ## Background
    ///
    /// In RIS, a query with the preselection `PACKAGE=/DMO/FLIGHT` and the desired
    /// facet, e.g. `GROUP`, returns contents of **all** sub-packages.
    ///
    /// In other words, you will get a number of groups even if the package `/DMO/FLIGHT`
    /// only consists of sub-packages. For this reason, the facet `PACKAGE=../DMO/FLIGHT`
    /// must be used, which is a special notation to request directly assigned objects.
    ///
    /// When child packages may exist, this method dispatches both requests concurrently.
    ///
    /// TODO: Ideally this should be done with a batch request once ADT supports it
    async fn load_package(
        &self,
        package: String,
        context: ExpansionContext,
        probe_child_packages: bool,
    ) -> Result<Vec<PreparedNode>, VfsError> {
        let direct_context = context.with(RepositoryPreselection::directly_assigned(&package));

        // Avoid doing needless work if we already know there are no child packages.
        if !probe_child_packages {
            return Ok(self.load_next_content_layer(direct_context, 0).await?.nodes);
        }

        // Even though package preselections are absolute, we cannot discard preselections
        // such as the owner or application component!
        let mut child_selection = context.preselections().to_vec();
        child_selection.push(RepositoryPreselection::new(
            RepositoryFacet::PACKAGE,
            &package,
        ));

        // Dispatch the futures concurrently to speed things up.
        // TODO: Batch this up!
        let child_packages = self.query_content(&child_selection, Some(&RepositoryFacet::PACKAGE));
        let direct_objects = self.load_next_content_layer(direct_context, 0);
        let (packages, objects) = try_join(child_packages, direct_objects).await?;

        let mut nodes = LoadedLayer::from_packages(packages, context)?.nodes;
        nodes.extend(objects.nodes);
        nodes.sort_by(PreparedNode::tree_order);
        Ok(nodes)
    }

    /// Applies the next configured facet, or returns objects when the chain ends.
    ///
    /// Adaptive levels below their threshold are skipped independently. This method loads
    /// objects and virtual folders exclusively. Packages have special handling.
    ///
    /// Because some layers may be skipped, this function might end up advancing multiple
    /// facet levels and issuing multiple requests, as the count of each level must still
    /// be obtained from the backend.
    async fn load_next_content_layer(
        &self,
        ctx: ExpansionContext,
        mut next_facet: usize,
    ) -> Result<LoadedLayer, VfsError> {
        let selection = ctx.preselections();
        loop {
            let Some(level) = ctx.facet_policy().levels().get(next_facet) else {
                // No more facets left, just get the objects
                let content = self.query_content(selection, None).await?;
                return Ok(LoadedLayer::from_objects(content));
            };

            // Content grouped by some facet, the object count decides whether we
            // keep it based on the adaptive treshold. Notably, these are not real
            // objects, they are virtual facets, so discarding them is not a big deal.
            let grouped = self.query_content(selection, Some(level.facet())).await?;
            if level.retains(grouped.object_count) {
                return Ok(LoadedLayer::from_folders(grouped, ctx, next_facet));
            }
            next_facet += 1;
        }
    }

    /// Fundamental internal helper that actually dispatches the adt request given
    /// the preselections and target facets. Short descriptions are always included.
    async fn query_content(
        &self,
        preselections: &[RepositoryPreselection],
        facet: Option<&RepositoryFacet>,
    ) -> Result<RepositoryContent, VfsError> {
        let mut builder = RepositoryContentQuery::builder()
            .operation(RepositoryContentOperation::Expand)
            .ignore_short_descriptions(false)
            .preselections(preselections);

        if let Some(facet) = facet {
            builder = builder.facet(facet.clone());
        }

        Ok(builder.build()?.execute(&self.inner.client).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FacetLevel;

    fn facet_context(expansion: ExpansionStrategy) -> ExpansionContext {
        match expansion {
            ExpansionStrategy::Facet { context, .. } => context,
            _ => panic!("expected a facet expansion"),
        }
    }

    #[test]
    fn facet_children_share_the_policy_and_append_their_selection() {
        let context = ExpansionContext::new(
            FacetPolicy::new([
                FacetLevel::always(RepositoryFacet::OWNER),
                FacetLevel::adaptive(RepositoryFacet::TYPE, 10),
            ]),
            vec![RepositoryPreselection::directly_assigned("$TMP")],
        );

        let expansion = context.child_facet(
            RepositoryPreselection::new(RepositoryFacet::OWNER, "DEVELOPER"),
            0,
            12,
            false,
        );
        let ExpansionStrategy::Facet {
            context: child,
            facet_index,
            object_count,
            has_children_of_same_facet,
        } = expansion
        else {
            panic!("expected a facet expansion");
        };

        assert!(Arc::ptr_eq(&context.facet_policy, &child.facet_policy));
        assert_eq!(context.preselections().len(), 1);
        assert_eq!(child.preselections().len(), 2);
        assert_eq!(facet_index, 0);
        assert_eq!(object_count, 12);
        assert!(!has_children_of_same_facet);
    }

    #[test]
    fn chained_facet_children_retain_the_complete_filter_path() {
        let context = ExpansionContext::new(
            FacetPolicy::grouped([
                RepositoryFacet::OWNER,
                RepositoryFacet::GROUP,
                RepositoryFacet::TYPE,
            ]),
            vec![
                RepositoryPreselection::directly_assigned("$TMP"),
                RepositoryPreselection::new(RepositoryFacet::FAVORITES, "$DEVELOPER"),
            ],
        );
        let owner = facet_context(context.child_facet(
            RepositoryPreselection::new(RepositoryFacet::OWNER, "DEVELOPER"),
            0,
            20,
            false,
        ));
        let group = facet_context(owner.child_facet(
            RepositoryPreselection::new(RepositoryFacet::GROUP, "SOURCE_LIBRARY"),
            1,
            20,
            false,
        ));
        let object_type = facet_context(group.child_facet(
            RepositoryPreselection::new(RepositoryFacet::TYPE, "CLAS"),
            2,
            10,
            false,
        ));

        let path = object_type
            .preselections()
            .iter()
            .map(|preselection| {
                (
                    preselection.facet().as_str(),
                    preselection.values()[0].as_str(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            path,
            [
                ("PACKAGE", "..$TMP"),
                ("FAV", "$DEVELOPER"),
                ("OWNER", "DEVELOPER"),
                ("GROUP", "SOURCE_LIBRARY"),
                ("TYPE", "CLAS"),
            ]
        );
    }

    #[test]
    fn package_children_share_the_context_without_adding_parent_packages() {
        let context = ExpansionContext::new(
            FacetPolicy::grouped([RepositoryFacet::OWNER]),
            vec![RepositoryPreselection::new(
                RepositoryFacet::API_STATE,
                "RELEASED",
            )],
        );

        let ExpansionStrategy::Package {
            package,
            context: child,
            has_child_packages,
        } = context.child_package("/ROOT/CHILD".to_owned(), true)
        else {
            panic!("expected a package expansion");
        };

        assert_eq!(package, "/ROOT/CHILD");
        assert!(has_child_packages);
        assert!(Arc::ptr_eq(&context.facet_policy, &child.facet_policy));
        assert_eq!(child.preselections(), context.preselections());
    }
}
