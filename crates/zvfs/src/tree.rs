//! Lazy repository-tree storage and expansion.
//!
//! The graph contains only nodes discovered so far. Each record combines a
//! public node snapshot with a private [`Expansion`] recipe for loading its
//! children. Graph locks cover short in-memory reads and writes only; ADT
//! requests run outside the graph lock and are deduplicated per node.

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
#[derive(Clone)]
pub struct RepositoryVfs {
    inner: Arc<Inner>,
}

struct Inner {
    client: Client<Ready>,
    facet_policy: FacetPolicy,
    root: NodeId,
    graph: RwLock<Graph>,
}

/// Mutable node storage for one VFS instance.
///
/// Public IDs include `scope`, while records use their compact numeric index.
/// Indices are never reused, so removed descendants cannot alias later nodes.
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

/// Describes how one directory obtains its immediate children.
#[derive(Clone)]
enum Expansion {
    /// A directory whose children were installed while constructing the VFS.
    Static,
    /// The top-level package hierarchy used by a system-library mount.
    PackageIndex {
        preselections: Vec<RepositoryPreselection>,
    },
    /// One package, expanded into child packages and directly assigned content.
    Package {
        package: String,
        preselections: Vec<RepositoryPreselection>,
    },
    /// An arbitrary caller-provided RIS selection.
    Selection {
        preselections: Vec<RepositoryPreselection>,
    },
    /// A virtual folder within the configured facet chain.
    Facet {
        preselections: Vec<RepositoryPreselection>,
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

/// Configures and creates a [`RepositoryVfs`].
pub struct RepositoryVfsBuilder {
    client: Client<Ready>,
    mounts: Vec<Mount>,
    facet_policy: FacetPolicy,
}

impl RepositoryVfsBuilder {
    fn new(client: Client<Ready>) -> Self {
        Self {
            client,
            mounts: Vec::new(),
            facet_policy: FacetPolicy::default(),
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

    /// Sets the virtual-facet policy used below all mounts.
    pub fn facet_policy(mut self, facet_policy: FacetPolicy) -> Self {
        self.facet_policy = facet_policy;
        self
    }

    /// Builds an in-memory tree without making an ADT request.
    pub fn build(self) -> RepositoryVfs {
        RepositoryVfs::from_builder(self)
    }
}

impl RepositoryVfs {
    /// Starts configuring a VFS backed by an already discovered ADT client.
    pub fn builder(client: Client<Ready>) -> RepositoryVfsBuilder {
        RepositoryVfsBuilder::new(client)
    }

    fn from_builder(builder: RepositoryVfsBuilder) -> Self {
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
                facet_policy: builder.facet_policy,
                root,
                graph: RwLock::new(graph),
            }),
        }
    }

    /// Returns the static root node identity.
    pub fn root(&self) -> NodeId {
        self.inner.root
    }

    /// Returns the configured facet policy.
    pub fn facet_policy(&self) -> &FacetPolicy {
        &self.inner.facet_policy
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
            Expansion::PackageIndex { preselections } => {
                let content = self
                    .query_content(&preselections, Some(RepositoryFacet::PACKAGE))
                    .await?;
                Ok(Loaded {
                    specs: package_specs(content, preselections),
                    object_count: None,
                    has_children_of_same_facet: None,
                })
            }
            Expansion::Package {
                package,
                preselections,
            } => Ok(Loaded {
                specs: self.load_package(package, preselections).await?,
                object_count: None,
                has_children_of_same_facet: None,
            }),
            Expansion::Selection { preselections } => {
                let layer = self.load_object_layer(preselections, 0).await?;
                Ok(Loaded {
                    specs: layer.specs,
                    object_count: None,
                    has_children_of_same_facet: None,
                })
            }
            Expansion::Facet {
                preselections,
                facet_index,
                has_children_of_same_facet,
                ..
            } => {
                if refresh {
                    return self.refresh_facet(preselections, facet_index).await;
                }
                let next_facet = if has_children_of_same_facet {
                    facet_index
                } else {
                    facet_index + 1
                };
                let layer = self.load_object_layer(preselections, next_facet).await?;
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
        preselections: Vec<RepositoryPreselection>,
        facet_index: usize,
    ) -> Result<Loaded, VfsError> {
        let Some(facet) = self.inner.facet_policy.facets().get(facet_index).cloned() else {
            let layer = self
                .load_object_layer(preselections, facet_index + 1)
                .await?;
            return Ok(Loaded {
                specs: layer.specs,
                object_count: Some(layer.object_count),
                has_children_of_same_facet: None,
            });
        };

        let selected_values = preselections
            .iter()
            .rev()
            .find(|preselection| preselection.facet() == &facet)
            .map(RepositoryPreselection::values)
            .unwrap_or_default();
        let mut same_facet = self
            .query_content(&preselections, Some(facet.clone()))
            .await?;
        same_facet.folders.retain(|folder| {
            folder.facet == facet && !selected_values.iter().any(|value| value == &folder.name)
        });

        if !same_facet.folders.is_empty() {
            let object_count = same_facet.object_count;
            let specs = if self
                .inner
                .facet_policy
                .minimum_objects()
                .is_some_and(|minimum| object_count < minimum)
            {
                let flat = self.query_content(&preselections, None).await?;
                self.content_specs(flat, preselections, None)
            } else {
                self.content_specs(same_facet, preselections, Some(facet_index))
            };
            return Ok(Loaded {
                specs,
                object_count: Some(object_count),
                has_children_of_same_facet: Some(true),
            });
        }

        let layer = self
            .load_object_layer(preselections, facet_index + 1)
            .await?;
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
        preselections: Vec<RepositoryPreselection>,
    ) -> Result<Vec<NodeSpec>, VfsError> {
        let mut child_selection = preselections.clone();
        child_selection.push(RepositoryPreselection::new(
            RepositoryFacet::PACKAGE,
            package.clone(),
        ));
        let mut direct_selection = preselections.clone();
        direct_selection.push(RepositoryPreselection::direct_package(package));

        let child_packages = self.query_content(&child_selection, Some(RepositoryFacet::PACKAGE));
        let direct_objects = self.load_object_layer(direct_selection, 0);
        let (child_packages, direct_objects) = try_join(child_packages, direct_objects).await?;

        let mut specs = package_specs(child_packages, preselections);
        specs.extend(direct_objects.specs);
        sort_specs(&mut specs);
        Ok(specs)
    }

    /// Applies the next configured facet, or returns objects when the chain ends.
    ///
    /// Adaptive policies first inspect the grouped response count and fall back
    /// to a flat object response when the layer is below the threshold.
    async fn load_object_layer(
        &self,
        preselections: Vec<RepositoryPreselection>,
        next_facet: usize,
    ) -> Result<LoadedLayer, VfsError> {
        let policy = &self.inner.facet_policy;
        let Some(facet) = policy.facets().get(next_facet).cloned() else {
            let content = self.query_content(&preselections, None).await?;
            return Ok(LoadedLayer {
                object_count: content.object_count,
                specs: self.content_specs(content, preselections, None),
            });
        };

        let grouped = self.query_content(&preselections, Some(facet)).await?;

        // did we hit the adaptive threshold? if not we just inline all objects
        if let Some(minimum) = policy.minimum_objects()
            && grouped.object_count < minimum
        {
            let content = self.query_content(&preselections, None).await?;
            return Ok(LoadedLayer {
                object_count: content.object_count,
                specs: self.content_specs(content, preselections, None),
            });
        }

        Ok(LoadedLayer {
            object_count: grouped.object_count,
            specs: self.content_specs(grouped, preselections, Some(next_facet)),
        })
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
        preselections: Vec<RepositoryPreselection>,
        facet_index: Option<usize>,
    ) -> Vec<NodeSpec> {
        let fallback_index = self.inner.facet_policy.facets().len();
        let mut specs = Vec::with_capacity(content.folders.len() + content.objects.len());

        for folder in content.folders {
            let mut child_selection = preselections.clone();
            child_selection.push(folder.preselection());
            specs.push(NodeSpec {
                label: folder_label(&folder.display_name, &folder.name),
                kind: NodeKind::Facet {
                    facet: folder.facet.to_string(),
                    value: folder.name,
                    object_count: folder.object_count,
                    has_children_of_same_facet: folder.has_children_of_same_facet,
                },
                expansion: Expansion::Facet {
                    preselections: child_selection,
                    facet_index: facet_index.unwrap_or(fallback_index),
                    object_count: folder.object_count,
                    has_children_of_same_facet: folder.has_children_of_same_facet,
                },
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
    match mount.target {
        MountTarget::SystemLibrary => NodeSpec {
            label: mount.label,
            kind: NodeKind::Mount {
                mount: MountKind::SystemLibrary,
            },
            expansion: Expansion::PackageIndex {
                preselections: Vec::new(),
            },
            object: None,
        },
        MountTarget::Package(package) => NodeSpec {
            label: mount.label,
            kind: NodeKind::Package {
                package: package.clone(),
                object_count: None,
            },
            expansion: Expansion::Package {
                package,
                preselections: Vec::new(),
            },
            object: None,
        },
        MountTarget::Selection(preselections) => NodeSpec {
            label: mount.label,
            kind: NodeKind::Mount {
                mount: MountKind::Selection,
            },
            expansion: Expansion::Selection { preselections },
            object: None,
        },
    }
}

fn package_specs(
    content: RepositoryContent,
    preselections: Vec<RepositoryPreselection>,
) -> Vec<NodeSpec> {
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
            expansion: Expansion::Package {
                package: folder.name,
                preselections: preselections.clone(),
            },
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
