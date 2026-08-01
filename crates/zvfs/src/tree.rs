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
//! [`Expansion`] strategy, which defines how that node will be expanded. Lock
//! retention is kept to an absolute minimum, locking the graph is only for read
//! and write access. During expansion (which inevitably invokes an I/O request),
//! a node-local mutex ensures data consistency on concurrent read requests without
//! keeping the rest of the graph locked.

use std::{collections::HashMap, sync::Arc};

use async_lock::Mutex;
use futures_util::future::try_join;
use parking_lot::RwLock;
use uuid::Uuid;
use zadt::{
    Client, Operation, Ready, RepositoryContent, RepositoryContentOperation,
    RepositoryContentQuery, RepositoryFacet, RepositoryObjectEntry, RepositoryPreselection,
};

use crate::{
    FacetPolicy, Mount, MountKind, Node, NodeId, NodeKind, ObjectNode, VfsError,
    config::MountTarget,
};

/// A cheap, shared handle to a lazy repository tree.
///
/// Much like the ADT client it holds, it is safe to hold
/// references to this VFS in various places.
#[derive(Clone)]
pub struct VirtualFileSystem {
    inner: Arc<Inner>,
}

struct Inner {
    client: Client<Ready>,
    root: NodeId,
    graph: RwLock<Graph>,
}

/// Mutable node storage for one VFS instance.
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

    fn index(&self, id: NodeId) -> Option<u64> {
        id.index_for(self.scope)
            .filter(|index| self.nodes.contains_key(index))
    }

    fn insert(&mut self, parent: Option<NodeId>, spec: NodeSpec) -> NodeId {
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
                    label: spec.label,
                    kind: spec.kind,
                },
                spec.expansion,
                spec.object,
            ),
        );
        id
    }
}

/// Internal state retained for a public node snapshot.
///
/// `children = None` means the node has not been expanded. The async load lock
/// serializes requests for this node without blocking expansion of other nodes.
struct NodeRecord {
    node: Node,
    expansion: Expansion,
    object: Option<RepositoryObjectEntry>,
    children: Option<Vec<u64>>,
    load: Arc<Mutex<()>>,
}

impl NodeRecord {
    fn new(node: Node, expansion: Expansion, object: Option<RepositoryObjectEntry>) -> Self {
        Self {
            node,
            expansion,
            object,
            children: None,
            load: Arc::new(Mutex::new(())),
        }
    }
}

/// Immutable mount configuration and the complete RIS filter path to one node.
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

    fn selected_values(&self, facet: &RepositoryFacet) -> &[String] {
        self.preselections
            .iter()
            .rev()
            .find(|preselection| preselection.facet() == facet)
            .map(RepositoryPreselection::values)
            .unwrap_or_default()
    }

    fn with_preselection(&self, preselection: RepositoryPreselection) -> Self {
        let mut context = self.clone();
        context.preselections.push(preselection);
        context
    }

    fn child_package(&self, package: String) -> Expansion {
        Expansion::Package {
            package,
            context: self.clone(),
        }
    }

    fn child_facet(
        &self,
        preselection: RepositoryPreselection,
        facet_index: usize,
        object_count: u32,
        has_children_of_same_facet: bool,
    ) -> Expansion {
        Expansion::Facet {
            context: self.with_preselection(preselection),
            facet_index,
            object_count,
            has_children_of_same_facet,
        }
    }
}

/// Describes how one directory obtains its immediate children.
#[derive(Clone)]
enum Expansion {
    /// A directory whose children were installed while constructing the VFS.
    Static,
    /// The top-level package hierarchy used by a system-library mount.
    PackageIndex { context: ExpansionContext },
    /// One package, expanded into child packages and directly assigned content.
    Package {
        package: String,
        context: ExpansionContext,
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
struct NodeSpec {
    label: String,
    kind: NodeKind,
    expansion: Expansion,
    object: Option<RepositoryObjectEntry>,
}

struct Loaded {
    specs: Vec<NodeSpec>,
    object_count: Option<u32>,
    has_children_of_same_facet: Option<bool>,
}

struct LoadedLayer {
    specs: Vec<NodeSpec>,
    object_count: u32,
}

/// Configures and creates a [`VirtualFileSystem`].
pub struct VirtualFileSystemBuilder {
    client: Client<Ready>,
    mounts: Vec<Mount>,
}

impl VirtualFileSystemBuilder {
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

    /// Builds an in-memory tree without making an ADT request.
    pub fn build(self) -> VirtualFileSystem {
        VirtualFileSystem::from_builder(self)
    }
}

impl VirtualFileSystem {
    /// Starts configuring a VFS backed by an already discovered ADT client.
    pub fn builder(client: Client<Ready>) -> VirtualFileSystemBuilder {
        VirtualFileSystemBuilder::new(client)
    }

    fn from_builder(builder: VirtualFileSystemBuilder) -> Self {
        let mut graph = Graph::new();
        let root = graph.insert(
            None,
            NodeSpec {
                label: "/".to_owned(),
                kind: NodeKind::Root,
                expansion: Expansion::Static,
                object: None,
            },
        );

        let mut children = Vec::with_capacity(builder.mounts.len());
        for mount in builder.mounts {
            children.push(
                insert_spec(&mut graph, root, mount_spec(mount))
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

        Self {
            inner: Arc::new(Inner {
                client: builder.client,
                root,
                graph: RwLock::new(graph),
            }),
        }
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
        let index = graph.index(id).ok_or(VfsError::UnknownNode(id))?;
        let record = graph.nodes.get(&index).ok_or(VfsError::UnknownNode(id))?;
        record.object.clone().ok_or(VfsError::NotObject(id))
    }

    /// Returns loaded children without starting an ADT request.
    pub fn cached_children(&self, id: NodeId) -> Result<Option<Vec<Node>>, VfsError> {
        let graph = self.inner.graph.read();
        let index = graph.index(id).ok_or(VfsError::UnknownNode(id))?;
        let record = graph.nodes.get(&index).ok_or(VfsError::UnknownNode(id))?;
        record
            .children
            .as_deref()
            .map(|children| snapshots(&graph, children))
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
            let index = graph.index(id).ok_or(VfsError::UnknownNode(id))?;
            let record = graph.nodes.get(&index).ok_or(VfsError::UnknownNode(id))?;
            if let Some(children) = &record.children {
                return snapshots(&graph, children);
            }
            if matches!(record.expansion, Expansion::Leaf) {
                return Err(VfsError::NotDirectory(id));
            }
            record.load.clone()
        };

        let _load_guard = load.lock().await;
        let expansion = {
            let graph = self.inner.graph.read();
            let index = graph.index(id).ok_or(VfsError::UnknownNode(id))?;
            let record = graph.nodes.get(&index).ok_or(VfsError::UnknownNode(id))?;

            // Second check after lock is relased!!!
            if let Some(children) = &record.children {
                return snapshots(&graph, children);
            }
            record.expansion.clone()
        };

        if matches!(expansion, Expansion::Static) {
            return Err(VfsError::NotRefreshable(id));
        }

        let loaded = self.load(expansion, false).await?;
        let mut graph = self.inner.graph.write();
        let index = graph.index(id).ok_or(VfsError::StaleNode(id))?;
        let children = insert_specs(&mut graph, id, loaded.specs);
        let record = graph.nodes.get_mut(&index).ok_or(VfsError::StaleNode(id))?;
        update_facet_state(
            record,
            loaded.object_count,
            loaded.has_children_of_same_facet,
        );
        record.children = Some(children.clone());
        snapshots(&graph, &children)
    }

    /// Reloads one directory and atomically replaces its cached descendants.
    ///
    /// Existing children remain visible while the ADT request is in flight. On
    /// success, their IDs become stale and newly loaded children replace them.
    pub async fn refresh(&self, id: NodeId) -> Result<Vec<Node>, VfsError> {
        let load = {
            let graph = self.inner.graph.read();
            let index = graph.index(id).ok_or(VfsError::UnknownNode(id))?;
            let record = graph.nodes.get(&index).ok_or(VfsError::UnknownNode(id))?;
            match record.expansion {
                Expansion::Static => return Err(VfsError::NotRefreshable(id)),
                Expansion::Leaf => return Err(VfsError::NotDirectory(id)),
                _ => record.load.clone(),
            }
        };

        let _load_guard = load.lock().await;

        let expansion = {
            let graph = self.inner.graph.read();
            let index = graph.index(id).ok_or(VfsError::StaleNode(id))?;
            graph
                .nodes
                .get(&index)
                .ok_or(VfsError::StaleNode(id))?
                .expansion
                .clone()
        };
        let loaded = self.load(expansion, true).await?;

        let mut graph = self.inner.graph.write();
        let index = graph.index(id).ok_or(VfsError::StaleNode(id))?;
        let old_children = graph
            .nodes
            .get(&index)
            .ok_or(VfsError::StaleNode(id))?
            .children
            .clone()
            .unwrap_or_default();
        remove_subtrees(&mut graph, old_children);

        let children = insert_specs(&mut graph, id, loaded.specs);
        let record = graph.nodes.get_mut(&index).ok_or(VfsError::StaleNode(id))?;
        update_facet_state(
            record,
            loaded.object_count,
            loaded.has_children_of_same_facet,
        );
        record.children = Some(children.clone());
        snapshots(&graph, &children)
    }

    /// Executes an expansion recipe and returns children not yet inserted into the graph.
    async fn load(&self, expansion: Expansion, refresh: bool) -> Result<Loaded, VfsError> {
        match expansion {
            Expansion::Static => unreachable!("static nodes have preloaded children"),
            Expansion::PackageIndex { context } => {
                let content = self
                    .query_content(context.preselections(), Some(RepositoryFacet::PACKAGE))
                    .await?;
                Ok(Loaded {
                    specs: package_specs(content, context),
                    object_count: None,
                    has_children_of_same_facet: None,
                })
            }
            Expansion::Package { package, context } => Ok(Loaded {
                specs: self.load_package(package, context).await?,
                object_count: None,
                has_children_of_same_facet: None,
            }),
            Expansion::Selection { context } => {
                let layer = self.load_object_layer(context, 0).await?;
                Ok(Loaded {
                    specs: layer.specs,
                    object_count: None,
                    has_children_of_same_facet: None,
                })
            }
            Expansion::Facet {
                context,
                facet_index,
                has_children_of_same_facet,
                ..
            } => {
                if refresh {
                    return self.refresh_facet(context, facet_index).await;
                }
                let next_facet = if has_children_of_same_facet {
                    facet_index
                } else {
                    facet_index + 1
                };
                let layer = self.load_object_layer(context, next_facet).await?;
                Ok(Loaded {
                    specs: layer.specs,
                    object_count: Some(layer.object_count),
                    has_children_of_same_facet: None,
                })
            }
            Expansion::Leaf => unreachable!("leaf expansion is rejected before loading"),
        }
    }

    /// Re-probes a facet so changes to same-facet hierarchy are discovered.
    async fn refresh_facet(
        &self,
        context: ExpansionContext,
        facet_index: usize,
    ) -> Result<Loaded, VfsError> {
        let Some(level) = context.facet_policy().levels().get(facet_index).cloned() else {
            let layer = self.load_object_layer(context, facet_index + 1).await?;
            return Ok(Loaded {
                specs: layer.specs,
                object_count: Some(layer.object_count),
                has_children_of_same_facet: None,
            });
        };
        let facet = level.facet().clone();

        let selected_values = context.selected_values(&facet);
        let mut same_facet = self
            .query_content(context.preselections(), Some(facet.clone()))
            .await?;
        same_facet.folders.retain(|folder| {
            folder.facet == facet && !selected_values.iter().any(|value| value == &folder.name)
        });

        if !same_facet.folders.is_empty() {
            let object_count = same_facet.object_count;
            let specs = if level.retains(object_count) {
                self.content_specs(same_facet, context, Some(facet_index))
            } else {
                self.load_object_layer(context, facet_index + 1)
                    .await?
                    .specs
            };
            return Ok(Loaded {
                specs,
                object_count: Some(object_count),
                has_children_of_same_facet: Some(true),
            });
        }

        let layer = self.load_object_layer(context, facet_index + 1).await?;
        Ok(Loaded {
            specs: layer.specs,
            object_count: Some(layer.object_count),
            has_children_of_same_facet: Some(false),
        })
    }

    /// Loads child packages and directly assigned content concurrently.
    async fn load_package(
        &self,
        package: String,
        context: ExpansionContext,
    ) -> Result<Vec<NodeSpec>, VfsError> {
        let mut child_selection = context.preselections().to_vec();
        child_selection.push(RepositoryPreselection::new(
            RepositoryFacet::PACKAGE,
            package.clone(),
        ));
        let direct_context =
            context.with_preselection(RepositoryPreselection::direct_package(package.clone()));

        let child_packages = self.query_content(&child_selection, Some(RepositoryFacet::PACKAGE));
        let direct_objects = self.load_object_layer(direct_context, 0);
        let (mut child_packages, direct_objects) = try_join(child_packages, direct_objects).await?;
        // RIS echoes the selected package in its own expansion.
        child_packages
            .folders
            .retain(|folder| folder.facet != RepositoryFacet::PACKAGE || folder.name != package);

        let mut specs = package_specs(child_packages, context);
        specs.extend(direct_objects.specs);
        sort_specs(&mut specs);
        Ok(specs)
    }

    /// Applies the next configured facet, or returns objects when the chain ends.
    ///
    /// Adaptive levels below their threshold are skipped independently.
    async fn load_object_layer(
        &self,
        context: ExpansionContext,
        mut next_facet: usize,
    ) -> Result<LoadedLayer, VfsError> {
        loop {
            let Some(level) = context.facet_policy().levels().get(next_facet).cloned() else {
                let content = self.query_content(context.preselections(), None).await?;
                return Ok(LoadedLayer {
                    object_count: content.object_count,
                    specs: self.content_specs(content, context, None),
                });
            };

            let grouped = self
                .query_content(context.preselections(), Some(level.facet().clone()))
                .await?;
            let object_count = grouped.object_count;
            if level.retains(object_count) {
                return Ok(LoadedLayer {
                    object_count,
                    specs: self.content_specs(grouped, context, Some(next_facet)),
                });
            }

            next_facet += 1;
        }
    }

    async fn query_content(
        &self,
        preselections: &[RepositoryPreselection],
        facet: Option<RepositoryFacet>,
    ) -> Result<RepositoryContent, VfsError> {
        let mut builder = RepositoryContentQuery::builder()
            .operation(RepositoryContentOperation::Expand)
            .ignore_short_descriptions(false);
        for preselection in preselections {
            builder = builder.preselection(preselection.clone());
        }
        if let Some(facet) = facet {
            builder = builder.facet(facet);
        }

        Ok(builder.build()?.execute(&self.inner.client).await?)
    }

    /// Converts one RIS response into graph-independent child specifications.
    fn content_specs(
        &self,
        content: RepositoryContent,
        context: ExpansionContext,
        facet_index: Option<usize>,
    ) -> Vec<NodeSpec> {
        let fallback_index = context.facet_policy().levels().len();
        let mut specs = Vec::with_capacity(content.folders.len() + content.objects.len());

        for folder in content.folders {
            let expansion = context.child_facet(
                folder.preselection(),
                facet_index.unwrap_or(fallback_index),
                folder.object_count,
                folder.has_children_of_same_facet,
            );
            specs.push(NodeSpec {
                label: folder_label(&folder.display_name, &folder.name),
                kind: NodeKind::Facet {
                    facet: folder.facet.to_string(),
                    value: folder.name,
                    object_count: folder.object_count,
                    has_children_of_same_facet: folder.has_children_of_same_facet,
                },
                expansion,
                object: None,
            });
        }
        specs.extend(content.objects.into_iter().map(object_spec));
        sort_specs(&mut specs);
        specs
    }
}

/// Converts root configuration into its initial node and expansion recipe.
fn mount_spec(mount: Mount) -> NodeSpec {
    let Mount {
        label,
        target,
        facet_policy,
    } = mount;
    match target {
        MountTarget::SystemLibrary => NodeSpec {
            label,
            kind: NodeKind::Mount {
                mount: MountKind::SystemLibrary,
            },
            expansion: Expansion::PackageIndex {
                context: ExpansionContext::new(facet_policy, Vec::new()),
            },
            object: None,
        },
        MountTarget::Package(package) => NodeSpec {
            label,
            kind: NodeKind::Package {
                package: package.clone(),
                object_count: None,
            },
            expansion: Expansion::Package {
                package,
                context: ExpansionContext::new(facet_policy, Vec::new()),
            },
            object: None,
        },
        MountTarget::Selection(preselections) => NodeSpec {
            label,
            kind: NodeKind::Mount {
                mount: MountKind::Selection,
            },
            expansion: Expansion::Selection {
                context: ExpansionContext::new(facet_policy, preselections),
            },
            object: None,
        },
    }
}

fn package_specs(content: RepositoryContent, context: ExpansionContext) -> Vec<NodeSpec> {
    let mut specs = content
        .folders
        .into_iter()
        .filter(|folder| folder.facet == RepositoryFacet::PACKAGE && !folder.is_direct_assignment())
        .map(|folder| NodeSpec {
            label: folder_label(&folder.display_name, &folder.name),
            kind: NodeKind::Package {
                package: folder.name.clone(),
                object_count: Some(folder.object_count),
            },
            expansion: context.child_package(folder.name),
            object: None,
        })
        .collect::<Vec<_>>();
    sort_specs(&mut specs);
    specs
}

fn object_spec(entry: RepositoryObjectEntry) -> NodeSpec {
    let object = ObjectNode {
        name: entry.name.clone(),
        package: entry.package.clone(),
        object_type: entry.object_type.to_string(),
        uri: entry.reference.uri().to_string(),
        virtual_workbench_uri: entry.virtual_workbench_uri.clone(),
        version: entry.version.clone(),
        expandable: entry.expandable,
        description: entry.description.clone(),
    };
    NodeSpec {
        label: entry.name.clone(),
        kind: NodeKind::Object { object },
        expansion: Expansion::Leaf,
        object: Some(entry),
    }
}

fn folder_label(display_name: &str, technical_name: &str) -> String {
    if display_name.is_empty() {
        technical_name.to_owned()
    } else {
        display_name.to_owned()
    }
}

fn sort_specs(specs: &mut [NodeSpec]) {
    specs.sort_by(|left, right| {
        node_rank(&left.kind)
            .cmp(&node_rank(&right.kind))
            .then_with(|| {
                left.label
                    .to_ascii_lowercase()
                    .cmp(&right.label.to_ascii_lowercase())
            })
            .then_with(|| left.label.cmp(&right.label))
    });
}

fn node_rank(kind: &NodeKind) -> u8 {
    match kind {
        NodeKind::Root | NodeKind::Mount { .. } => 0,
        NodeKind::Package { .. } => 1,
        NodeKind::Facet { .. } => 2,
        NodeKind::Object { .. } => 3,
    }
}

fn insert_specs(graph: &mut Graph, parent: NodeId, specs: Vec<NodeSpec>) -> Vec<u64> {
    specs
        .into_iter()
        .map(|spec| {
            insert_spec(graph, parent, spec)
                .index_for(graph.scope)
                .expect("a newly inserted node belongs to its graph")
        })
        .collect()
}

fn insert_spec(graph: &mut Graph, parent: NodeId, spec: NodeSpec) -> NodeId {
    graph.insert(Some(parent), spec)
}

fn snapshots(graph: &Graph, ids: &[u64]) -> Result<Vec<Node>, VfsError> {
    ids.iter()
        .map(|id| {
            graph
                .nodes
                .get(id)
                .map(|record| record.node.clone())
                .ok_or(VfsError::UnknownNode(NodeId::new(graph.scope, *id)))
        })
        .collect()
}

fn remove_subtrees(graph: &mut Graph, roots: Vec<u64>) {
    let mut pending = roots;
    while let Some(id) = pending.pop() {
        if let Some(record) = graph.nodes.remove(&id)
            && let Some(children) = record.children
        {
            pending.extend(children);
        }
    }
}

fn update_facet_state(
    record: &mut NodeRecord,
    object_count: Option<u32>,
    has_children_of_same_facet: Option<bool>,
) {
    if let NodeKind::Facet {
        object_count: count,
        has_children_of_same_facet: has_children,
        ..
    } = &mut record.node.kind
    {
        if let Some(object_count) = object_count {
            *count = object_count;
        }
        if let Some(has_children_of_same_facet) = has_children_of_same_facet {
            *has_children = has_children_of_same_facet;
        }
    }
    if let Expansion::Facet {
        object_count: count,
        has_children_of_same_facet: has_children,
        ..
    } = &mut record.expansion
    {
        if let Some(object_count) = object_count {
            *count = object_count;
        }
        if let Some(has_children_of_same_facet) = has_children_of_same_facet {
            *has_children = has_children_of_same_facet;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FacetLevel;

    fn facet_context(expansion: Expansion) -> ExpansionContext {
        match expansion {
            Expansion::Facet { context, .. } => context,
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
            vec![RepositoryPreselection::direct_package("$TMP")],
        );

        let expansion = context.child_facet(
            RepositoryPreselection::new(RepositoryFacet::OWNER, "DEVELOPER"),
            0,
            12,
            false,
        );
        let Expansion::Facet {
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
        assert_eq!(
            child.selected_values(&RepositoryFacet::OWNER),
            ["DEVELOPER"]
        );
        assert_eq!(facet_index, 0);
        assert_eq!(object_count, 12);
        assert!(!has_children_of_same_facet);
    }

    #[test]
    fn selected_values_use_the_deepest_selection_of_the_same_facet() {
        let context = ExpansionContext::new(
            FacetPolicy::grouped([RepositoryFacet::OWNER]),
            vec![RepositoryPreselection::new(RepositoryFacet::OWNER, "DEVELOPER").include("ALICE")],
        );
        let child = facet_context(context.child_facet(
            RepositoryPreselection::new(RepositoryFacet::OWNER, "ALICE"),
            0,
            1,
            false,
        ));

        assert_eq!(
            context.selected_values(&RepositoryFacet::OWNER),
            ["DEVELOPER", "ALICE"]
        );
        assert_eq!(child.selected_values(&RepositoryFacet::OWNER), ["ALICE"]);
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
                RepositoryPreselection::direct_package("$TMP"),
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

        let Expansion::Package {
            package,
            context: child,
        } = context.child_package("/ROOT/CHILD".to_owned())
        else {
            panic!("expected a package expansion");
        };

        assert_eq!(package, "/ROOT/CHILD");
        assert!(Arc::ptr_eq(&context.facet_policy, &child.facet_policy));
        assert_eq!(child.preselections(), context.preselections());
    }
}
