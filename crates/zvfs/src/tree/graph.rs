//! Mutable node storage and semantic child reconciliation.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_lock::Mutex;
use uuid::Uuid;
use zadt::{AdtUri, RepositoryObjectEntry};

use super::expand::{ExpansionStrategy, PreparedNode};
use crate::{Node, NodeId, NodeKind, VfsError};

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
pub(super) struct Graph {
    pub(super) scope: Uuid,
    next_index: u64,
    pub(super) nodes: HashMap<u64, NodeRecord>,
}

impl Graph {
    pub(super) fn new() -> Self {
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
    pub(super) fn index(&self, id: NodeId) -> Option<u64> {
        id.index_for(self.scope)
            .filter(|index| self.nodes.contains_key(index))
    }

    /// Returns the live record for a node ID belonging to this tree.
    pub(super) fn record(&self, id: NodeId) -> Option<&NodeRecord> {
        let index = id.index_for(self.scope)?;
        self.nodes.get(&index)
    }

    /// Returns the live record mutably for a node ID belonging to this tree.
    pub(super) fn mut_record(&mut self, id: NodeId) -> Option<&mut NodeRecord> {
        let index = id.index_for(self.scope)?;
        self.nodes.get_mut(&index)
    }

    /// Inserts one prepared node and assigns it the next scoped identity.
    ///
    /// Indices are consumed monotonically and remain reserved after removal.
    /// This method does not validate `parent`. Callers inserting children from
    /// asynchronously loaded data must use [`Graph::try_insert_children`].
    pub(super) fn insert(&mut self, parent: Option<NodeId>, node: PreparedNode) -> NodeId {
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

    /// Inserts prepared children if `parent` still has the expected generation.
    ///
    /// Parent validation happens before any identities are allocated. This
    /// prevents an ancestor refresh that completed while backend work was in
    /// flight from leaving newly inserted children without a live parent.
    ///
    /// Returns the childrens internal indices in iteration order without
    /// allocating identities when the parent changed while the load was in flight.
    pub(super) fn try_insert_children(
        &mut self,
        parent: NodeId,
        generation: u64,
        children: Vec<PreparedNode>,
    ) -> Result<Vec<u64>, VfsError> {
        let record = self.record(parent).ok_or(VfsError::StaleNode(parent))?;
        if record.generation != generation {
            return Err(VfsError::StaleNode(parent));
        }
        Self::validate_child_identities(parent, &children)?;

        Ok(children
            .into_iter()
            .map(|node| {
                self.insert(Some(parent), node)
                    .index_for(self.scope)
                    .expect("a newly inserted node belongs to its graph")
            })
            .collect())
    }

    /// Reconciles one freshly loaded layer against the current immediate children.
    /// Matching records keep their IDs, load gates, and compatible descendant caches.
    pub(super) fn reconcile_children(
        &mut self,
        parent: NodeId,
        generation: u64,
        children: Vec<PreparedNode>,
    ) -> Result<Vec<u64>, VfsError> {
        let record = self.record(parent).ok_or(VfsError::StaleNode(parent))?;
        if record.generation != generation {
            return Err(VfsError::StaleNode(parent));
        }
        Self::validate_child_identities(parent, &children)?;

        let current_children = record.children.clone().unwrap_or_default();
        let mut current_by_identity = HashMap::with_capacity(current_children.len());
        for index in current_children {
            let record = self
                .nodes
                .get(&index)
                .expect("child indices always reference existing records");
            let identity = SemanticKey::from_kind(&record.node.kind)
                .expect("loaded children always have semantic identities");
            let previous = current_by_identity.insert(identity, index);
            debug_assert!(previous.is_none(), "child identities are unique per parent");
        }

        let mut reconciled = Vec::with_capacity(children.len());
        let mut invalidated_descendants = Vec::new();
        for child in children {
            let identity = SemanticKey::from_kind(&child.kind)
                .expect("loaded children always have semantic identities");
            if let Some(index) = current_by_identity.remove(&identity) {
                let record = self
                    .nodes
                    .get_mut(&index)
                    .expect("a reconciled child remains present");
                if let Some(descendants) = record.apply_prepared(child) {
                    invalidated_descendants.extend(descendants);
                }
                reconciled.push(index);
            } else {
                reconciled.push(
                    self.insert(Some(parent), child)
                        .index_for(self.scope)
                        .expect("a newly inserted node belongs to its graph"),
                );
            }
        }

        invalidated_descendants.extend(current_by_identity.into_values());
        self.remove_subtrees(invalidated_descendants);
        Ok(reconciled)
    }

    /// An internal helper to ensure entries are never duplicated based on
    /// their AdtUri, which uniquely identifies the object.
    fn validate_child_identities(
        parent: NodeId,
        children: &[PreparedNode],
    ) -> Result<(), VfsError> {
        let mut identities = HashSet::with_capacity(children.len());
        for child in children {
            let identity = SemanticKey::from_kind(&child.kind)
                .expect("loaded children always have semantic identities");
            if !identities.insert(identity.clone()) {
                return Err(VfsError::DuplicateChildIdentity {
                    parent,
                    identity: identity.description(),
                });
            }
        }
        Ok(())
    }

    /// Returns owned snapshots for the given internal node indices.
    ///
    /// Nodes are cloned so the returned values do not borrow the graph or extend
    /// the lifetime of its lock guard. Input order is preserved, and later graph
    /// updates do not affect the snapshots.
    pub(super) fn node_snapshots(&self, ids: &[u64]) -> Result<Vec<Node>, VfsError> {
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
pub(super) struct NodeRecord {
    pub(super) node: Node,
    pub(super) expansion: ExpansionStrategy,
    pub(super) object: Option<RepositoryObjectEntry>,
    pub(super) children: Option<Vec<u64>>,
    pub(super) load: Arc<Mutex<()>>,
    /// Changes when an ancestor reconciliation updates this records load inputs.
    pub(super) generation: u64,
    /// Changes when an explicit refresh of this record is committed.
    pub(super) refresh_revision: u64,
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
            generation: 0,
            refresh_revision: 0,
        }
    }

    /// Updates a retained records metadata while preserving its stable identity.
    /// Descendants remain cached only when the new expansion has the same shape.
    fn apply_prepared(&mut self, prepared: PreparedNode) -> Option<Vec<u64>> {
        let invalidated = if self.expansion.cache_compatible(&prepared.expansion) {
            None
        } else {
            self.children.take()
        };

        self.node.label = prepared.label;
        self.node.kind = prepared.kind;
        self.expansion = prepared.expansion;
        self.object = prepared.object;
        self.generation = self
            .generation
            .checked_add(1)
            .expect("a VFS node generation cannot overflow");
        invalidated
    }

    /// Adds the children to the node and updates the metadata if applicable.
    /// This is needed after a refresh was perfomed, as facet state may change.
    pub(super) fn install_children(
        &mut self,
        children: Vec<u64>,
        object_count: Option<u32>,
        has_children_of_same_facet: Option<bool>,
    ) {
        self.update_facet_state(object_count, has_children_of_same_facet);
        self.children = Some(children);
    }

    /// Advances the refresh revision. This is done to prevent concurrent refresh
    /// requests, the later request is simply discarded.
    pub(super) fn advance_refresh_revision(&mut self) {
        self.refresh_revision = self
            .refresh_revision
            .checked_add(1)
            .expect("a VFS node refresh revision cannot overflow");
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

/// Stable identity for one child within a materialized directory layer.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum SemanticKey {
    Package(AdtUri),
    Facet { facet: String, value: String },
    Object(AdtUri),
}

impl SemanticKey {
    fn from_kind(kind: &NodeKind) -> Option<Self> {
        match kind {
            NodeKind::Package { uri, .. } => Some(Self::Package(uri.clone())),
            NodeKind::Facet { facet, value, .. } => Some(Self::Facet {
                facet: facet.clone(),
                value: value.clone(),
            }),
            NodeKind::Object { object } => Some(Self::Object(object.uri.clone())),
            NodeKind::Root | NodeKind::Mount { .. } => None,
        }
    }

    fn description(&self) -> String {
        match self {
            Self::Package(uri) => format!("package:{}", uri.as_str()),
            Self::Facet { facet, value } => format!("facet:{facet}:{value}"),
            Self::Object(uri) => format!("object:{}", uri.as_str()),
        }
    }
}
