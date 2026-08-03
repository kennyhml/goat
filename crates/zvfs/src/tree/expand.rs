//! Repository expansion strategies and RIS response conversion.

use std::{cmp::Ordering, sync::Arc};

use async_lock::MutexGuardArc;
use futures_util::future::try_join_all;
use zadt::{
    BatchKey, Batched, Client, Operation, Package, Ready, RepositoryContent,
    RepositoryContentOperation, RepositoryContentQuery, RepositoryFacet, RepositoryObjectEntry,
    RepositoryPreselection, RepositoryVirtualFolder,
};

use super::VirtualRepositoryTree;
use crate::{
    FacetPolicy, Mount, MountKind, NodeId, NodeKind, ObjectNode, VfsError, config::MountTarget,
};

/// The execution state of an expansion. While [`ExpansionStrategy`] permanently
/// tells its associated node how it can expand, the [`Expansion`] holds the
/// concrete plan to actually perform that expansion. This decouples orchestrating
/// the expansions from actually performing the requests, allowing us to efficiently
/// execute them concurrently later.
///
/// One strategy can produce multiple independent expansions. A package, for
/// example, can produce one expansion for child packages and another for directly
/// assigned content. Their requests can execute concurrently and their
/// [`PreparedChildren`] are merged after both expansions complete.
///
/// Each expansion follows a request/advance cycle. [`Expansion::request`] builds
/// the query for its current state without mutating it. The corresponding response
/// must then be passed back to the same expansion through [`Expansion::advance`].
/// Advancing either completes the expansion or updates its cursor so another query
/// can be issued in the next wave.
///
/// The process looks roughly like this:
///
/// NodeRecord
/// └── ExpansionStrategy (persistent expansion definition)
///         │
///         ▼
///     Expansion (temporary execution state)
///         │ request / advance
///         ▼
///     PreparedChildren
///
/// When several expansions are driven together, completed expansions retain their
/// prepared children while only incomplete adaptive expansions enter the next wave:
///
/// ```text
/// wave 1: [child packages, direct TYPE, sibling TYPE]
///                                │
///                                └── adaptive TYPE skipped
/// wave 2: [direct objects]
/// ```
enum Expansion {
    /// Top-level package hierarchy.
    PackageIndex { context: ExpansionContext },
    /// Child packages belonging to one package.
    ChildPackages {
        context: ExpansionContext,
        selection: Vec<RepositoryPreselection>,
    },
    /// Content directly assigned to one package.
    Package { cursor: ContentCursor },
    /// Content selected through a mount or facet.
    Content {
        cursor: ContentCursor,
        retain_object_count: bool,
    },
}

impl Expansion {
    /// Converts a persistent node strategy into independently executable expansions.
    ///
    /// Most strategies produce one expansion. Packages can produce separate child
    /// package and direct-content expansions, allowing both requests to run concurrently.
    /// Static nodes and leaves have no backend expansion and therefore produce none.
    fn prepare(strategy: ExpansionStrategy) -> Vec<Self> {
        match strategy {
            ExpansionStrategy::Static | ExpansionStrategy::Leaf => Vec::new(),
            ExpansionStrategy::PackageIndex { context } => {
                vec![Self::PackageIndex { context }]
            }
            ExpansionStrategy::Package {
                package,
                context,
                has_child_packages,
            } => {
                let mut expansions = Vec::with_capacity(if has_child_packages { 2 } else { 1 });

                // If there are child packages, we need both expansions.
                if has_child_packages {
                    let mut selection = context.preselections().to_vec();
                    selection.push(RepositoryPreselection::new(
                        RepositoryFacet::PACKAGE,
                        &package,
                    ));
                    expansions.push(Self::ChildPackages {
                        context: context.clone(),
                        selection,
                    });
                }
                expansions.push(Self::Package {
                    cursor: ContentCursor {
                        context: context.with(RepositoryPreselection::directly_assigned(&package)),
                        next_facet: 0,
                    },
                });
                expansions
            }
            ExpansionStrategy::Selection { context } => vec![Self::Content {
                cursor: ContentCursor {
                    context,
                    next_facet: 0,
                },
                retain_object_count: false,
            }],
            ExpansionStrategy::Facet {
                context,
                facet_index,
                has_children_of_same_facet,
                ..
            } => vec![Self::Content {
                cursor: ContentCursor {
                    context,
                    next_facet: if has_children_of_same_facet {
                        facet_index
                    } else {
                        facet_index + 1
                    },
                },
                retain_object_count: true,
            }],
        }
    }

    /// Builds the next query for this expansion's current state.
    ///
    /// This does not mutate the expansion. The decoded response must be routed back
    /// to this same value through [`Expansion::advance`] before another request is
    /// built for it.
    fn request(&self) -> Result<RepositoryContentQuery, VfsError> {
        match self {
            Self::PackageIndex { context } => {
                content_query(context.preselections(), Some(&RepositoryFacet::PACKAGE))
            }
            Self::ChildPackages { selection, .. } => {
                content_query(selection, Some(&RepositoryFacet::PACKAGE))
            }
            Self::Package { cursor } | Self::Content { cursor, .. } => cursor.request(),
        }
    }

    /// Applies the response produced by the preceding [`Expansion::request`].
    ///
    /// `Some` completes the expansion and yields children ready for graph insertion.
    /// `None` means an adaptive facet was not retained; the internal cursor has moved
    /// to the next level and [`Expansion::request`] must be called again.
    fn advance(
        &mut self,
        content: RepositoryContent,
    ) -> Result<Option<PreparedChildren>, VfsError> {
        match self {
            Self::PackageIndex { context } | Self::ChildPackages { context, .. } => {
                Ok(Some(PreparedChildren {
                    nodes: LoadedLayer::from_packages(content, context.clone())?.nodes,
                    object_count: None,
                    has_children_of_same_facet: None,
                }))
            }
            Self::Package { cursor } => Ok(cursor.advance(content).map(|layer| PreparedChildren {
                nodes: layer.nodes,
                object_count: None,
                has_children_of_same_facet: None,
            })),
            Self::Content {
                cursor,
                retain_object_count,
            } => {
                let Some(layer) = cursor.advance(content) else {
                    return Ok(None);
                };
                Ok(Some(PreparedChildren {
                    object_count: retain_object_count.then_some(layer.object_count),
                    nodes: layer.nodes,
                    has_children_of_same_facet: None,
                }))
            }
        }
    }
}

/// Describes how one directory obtains its immediate children.
#[derive(Clone)]
pub(super) enum ExpansionStrategy {
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

impl ExpansionStrategy {
    /// Returns whether descendants loaded with `self` remain valid for `other`.
    ///
    /// Reconciliation may retain a node by semantic identity even when its expansion
    /// shape changes. For example, a facet that previously advanced to `TYPE` may now
    /// have another `APPLICATION_COMPONENT` level, or an adaptive threshold may add or
    /// remove a facet level. In those cases, the node ID remains stable, but its cached
    /// descendants must be discarded.
    ///
    /// In reality, it is extremely unlikely for this to happen.
    pub(super) fn cache_compatible(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Static, Self::Static) | (Self::Leaf, Self::Leaf) => true,
            (Self::PackageIndex { context: left }, Self::PackageIndex { context: right })
            | (Self::Selection { context: left }, Self::Selection { context: right }) => {
                left == right
            }
            (
                Self::Package {
                    package: left_package,
                    context: left_context,
                    has_child_packages: left_has_children,
                },
                Self::Package {
                    package: right_package,
                    context: right_context,
                    has_child_packages: right_has_children,
                },
            ) => {
                left_package == right_package
                    && left_context == right_context
                    && left_has_children == right_has_children
            }
            (
                Self::Facet {
                    context: left_context,
                    facet_index: left_index,
                    object_count: left_count,
                    has_children_of_same_facet: left_has_children,
                },
                Self::Facet {
                    context: right_context,
                    facet_index: right_index,
                    object_count: right_count,
                    has_children_of_same_facet: right_has_children,
                },
            ) => {
                left_context == right_context
                    && left_index == right_index
                    && left_count == right_count
                    && left_has_children == right_has_children
            }
            _ => false,
        }
    }
}

/// Immutable mount configuration and the complete RIS filter path to one node.
///
/// The vector of preselections must be cloned per node.
#[derive(Clone, Eq, PartialEq)]
pub(super) struct ExpansionContext {
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

/// A node prepared off-graph and assigned an identity when committed.
pub(super) struct PreparedNode {
    pub(super) label: String,
    pub(super) kind: NodeKind,
    pub(super) expansion: ExpansionStrategy,
    pub(super) object: Option<RepositoryObjectEntry>,
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

    pub(super) fn from_mount(mount: Mount, client: &Client<Ready>) -> Result<Self, VfsError> {
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

pub(super) struct PreparedChildren {
    pub(super) nodes: Vec<PreparedNode>,
    pub(super) object_count: Option<u32>,
    pub(super) has_children_of_same_facet: Option<bool>,
}

impl PreparedChildren {
    fn merge(parts: Vec<Self>) -> Self {
        let mut merged = Self {
            nodes: Vec::new(),
            object_count: None,
            has_children_of_same_facet: None,
        };
        for mut part in parts {
            debug_assert!(merged.object_count.is_none() || part.object_count.is_none());
            debug_assert!(
                merged.has_children_of_same_facet.is_none()
                    || part.has_children_of_same_facet.is_none()
            );
            merged.object_count = merged.object_count.or(part.object_count);
            merged.has_children_of_same_facet = merged
                .has_children_of_same_facet
                .or(part.has_children_of_same_facet);
            merged.nodes.append(&mut part.nodes);
        }
        merged.nodes.sort_by(PreparedNode::tree_order);
        merged
    }
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

struct ContentCursor {
    context: ExpansionContext,
    next_facet: usize,
}

impl ContentCursor {
    fn request(&self) -> Result<RepositoryContentQuery, VfsError> {
        let facet = self
            .context
            .facet_policy()
            .levels()
            .get(self.next_facet)
            .map(|level| level.facet());
        content_query(self.context.preselections(), facet)
    }

    fn advance(&mut self, content: RepositoryContent) -> Option<LoadedLayer> {
        let Some(level) = self.context.facet_policy().levels().get(self.next_facet) else {
            return Some(LoadedLayer::from_objects(content));
        };

        if level.retains(content.object_count) {
            return Some(LoadedLayer::from_folders(
                content,
                self.context.clone(),
                self.next_facet,
            ));
        }

        self.next_facet += 1;
        None
    }
}

struct PendingPreload {
    node: NodeId,
    generation: u64,
    expansions: Vec<Expansion>,
    prepared: Vec<PreparedChildren>,
    _load: MutexGuardArc<()>,
}

impl VirtualRepositoryTree {
    /// Executes an expansion strategy and returns children not yet inserted into the graph.
    pub(super) async fn load(
        &self,
        expansion: ExpansionStrategy,
        refresh: bool,
    ) -> Result<PreparedChildren, VfsError> {
        if !refresh {
            return self.execute_expansion(expansion).await;
        }

        match expansion {
            ExpansionStrategy::Static => unreachable!("static nodes have preloaded children"),
            // Loading of all packages, likely from the system library
            ExpansionStrategy::PackageIndex { context } => {
                self.execute_expansion(ExpansionStrategy::PackageIndex { context })
                    .await
            }
            // Loading of the contents of a package, including directly assigned objects
            // and child packages if applicable.
            ExpansionStrategy::Package {
                package: pkg,
                context: ctx,
                has_child_packages: mut probe_child_packages,
            } => {
                probe_child_packages |= refresh;
                self.execute_expansion(ExpansionStrategy::Package {
                    package: pkg,
                    context: ctx,
                    has_child_packages: probe_child_packages,
                })
                .await
            }
            // Loading via a custom selection strategy
            ExpansionStrategy::Selection { context } => {
                self.execute_expansion(ExpansionStrategy::Selection { context })
                    .await
            }
            // Loading of a facet using its facet index to point to the current policy
            ExpansionStrategy::Facet {
                context,
                facet_index,
                ..
            } => {
                if refresh {
                    return self.refresh_facet(context, facet_index).await;
                }
                unreachable!("non-refresh facet loads use prepared expansions")
            }
            ExpansionStrategy::Leaf => unreachable!("leaf expansion is rejected before loading"),
        }
    }

    async fn execute_expansion(
        &self,
        strategy: ExpansionStrategy,
    ) -> Result<PreparedChildren, VfsError> {
        let mut pending = Expansion::prepare(strategy);
        assert!(
            !pending.is_empty(),
            "static and leaf expansions are rejected before loading"
        );
        let mut prepared = Vec::new();
        loop {
            let requests = pending
                .iter()
                .map(Expansion::request)
                .collect::<Result<Vec<_>, _>>()?;
            let responses = self.batch_execute_requests(requests).await?;
            let mut next = Vec::new();
            for (mut expansion, response) in pending.into_iter().zip(responses) {
                match expansion.advance(response)? {
                    Some(children) => prepared.push(children),
                    None => next.push(expansion),
                }
            }
            if next.is_empty() {
                return Ok(PreparedChildren::merge(prepared));
            }
            pending = next;
        }
    }

    /// Executes the provided [`RepositoryContentQuery`] either in batch or in
    /// single mode depending on the number of queries needed. While the effect
    /// is negligible for a small set of requests, it can noticably improve
    /// performance for preloading a bigger set of nodes.
    async fn batch_execute_requests(
        &self,
        requests: Vec<RepositoryContentQuery>,
    ) -> Result<Vec<RepositoryContent>, VfsError> {
        if requests.len() > 1 && self.inner.client.batch().is_ok() {
            return Ok(requests.batched(&self.inner.client).await?);
        }

        // Fall back to sequential requests if batching it not supported
        try_join_all(requests.into_iter().map(|request| async move {
            Ok::<_, VfsError>(request.execute(&self.inner.client).await?)
        }))
        .await
    }

    pub(super) async fn preload_children(&self, children: Vec<NodeId>) {
        let mut pending = Vec::new();
        for node in children {
            let load = {
                let graph = self.inner.graph.read();
                let Some(record) = graph.record(node) else {
                    continue;
                };
                if record.children.is_some() {
                    continue;
                }
                record.load.clone()
            };
            let Some(load_guard) = load.try_lock_arc() else {
                continue;
            };
            let Some((generation, strategy)) = ({
                let graph = self.inner.graph.read();
                graph.record(node).and_then(|record| {
                    record
                        .children
                        .is_none()
                        .then(|| (record.generation, record.expansion.clone()))
                })
            }) else {
                continue;
            };
            let expansions = Expansion::prepare(strategy);
            if expansions.is_empty() {
                continue;
            }
            pending.push(PendingPreload {
                node,
                generation,
                expansions,
                prepared: Vec::new(),
                _load: load_guard,
            });
        }

        while !pending.is_empty() {
            let Ok(mut batch) = self.inner.client.batch() else {
                return;
            };
            let mut routes: Vec<(usize, usize, BatchKey<RepositoryContent>)> = Vec::new();
            let mut failed = vec![false; pending.len()];

            for (index, preload) in pending.iter().enumerate() {
                for (expansion_index, expansion) in preload.expansions.iter().enumerate() {
                    match expansion.request() {
                        Ok(request) => {
                            routes.push((index, expansion_index, batch.push(request)));
                        }
                        Err(_) => failed[index] = true,
                    }
                }
            }

            if routes.is_empty() {
                return;
            }
            let Ok(mut batch_responses) = batch.execute(&self.inner.client).await else {
                return;
            };
            let mut responses = pending
                .iter()
                .map(|preload| {
                    (0..preload.expansions.len())
                        .map(|_| None)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            for (index, expansion_index, key) in routes {
                match batch_responses.take(key) {
                    Ok(response) => responses[index][expansion_index] = Some(response),
                    Err(_) => failed[index] = true,
                }
            }

            let mut next = Vec::new();
            for (index, mut preload) in pending.into_iter().enumerate() {
                if failed[index] {
                    continue;
                }
                let mut next_expansions = Vec::new();
                let expansion_responses = std::mem::take(&mut responses[index]);
                let mut advance_failed = false;
                for (mut expansion, response) in
                    preload.expansions.into_iter().zip(expansion_responses)
                {
                    let Some(response) = response else {
                        advance_failed = true;
                        break;
                    };
                    match expansion.advance(response) {
                        Ok(Some(prepared)) => preload.prepared.push(prepared),
                        Ok(None) => next_expansions.push(expansion),
                        Err(_) => {
                            advance_failed = true;
                            break;
                        }
                    }
                }
                if advance_failed {
                    continue;
                }
                if next_expansions.is_empty() {
                    let prepared = PreparedChildren::merge(preload.prepared);
                    self.commit_preload(preload.node, preload.generation, prepared);
                } else {
                    preload.expansions = next_expansions;
                    next.push(preload);
                }
            }
            pending = next;
        }
    }

    fn commit_preload(&self, node: NodeId, generation: u64, prepared: PreparedChildren) {
        let mut graph = self.inner.graph.write();
        let _ = graph.install_loaded_children(
            node,
            generation,
            prepared.nodes,
            prepared.object_count,
            prepared.has_children_of_same_facet,
        );
    }

    /// Re-probes a facet so changes to same-facet hierarchy are discovered.
    ///
    /// This needs special care as, depending on the facet, its possible that the
    /// `has_children_of_same_facet` on our side no longer matches the actual state.
    async fn refresh_facet(
        &self,
        ctx: ExpansionContext,
        facet_index: usize,
    ) -> Result<PreparedChildren, VfsError> {
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
            return Ok(PreparedChildren {
                nodes: layer.nodes,
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
            return Ok(PreparedChildren {
                nodes: specs,
                object_count: Some(object_count),
                has_children_of_same_facet: Some(true),
            });
        }

        let layer = self.load_next_content_layer(ctx, facet_index + 1).await?;
        Ok(PreparedChildren {
            nodes: layer.nodes,
            object_count: Some(layer.object_count),
            has_children_of_same_facet: Some(false),
        })
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
        next_facet: usize,
    ) -> Result<LoadedLayer, VfsError> {
        let mut cursor = ContentCursor {
            context: ctx,
            next_facet,
        };
        loop {
            let content = cursor.request()?.execute(&self.inner.client).await?;
            if let Some(layer) = cursor.advance(content) {
                return Ok(layer);
            }
        }
    }

    /// Fundamental internal helper that actually dispatches the adt request given
    /// the preselections and target facets. Short descriptions are always included.
    async fn query_content(
        &self,
        preselections: &[RepositoryPreselection],
        facet: Option<&RepositoryFacet>,
    ) -> Result<RepositoryContent, VfsError> {
        Ok(content_query(preselections, facet)?
            .execute(&self.inner.client)
            .await?)
    }
}

fn content_query(
    preselections: &[RepositoryPreselection],
    facet: Option<&RepositoryFacet>,
) -> Result<RepositoryContentQuery, VfsError> {
    let mut builder = RepositoryContentQuery::builder()
        .operation(RepositoryContentOperation::Expand)
        .ignore_short_descriptions(false)
        .preselections(preselections);

    if let Some(facet) = facet {
        builder = builder.facet(facet.clone());
    }

    Ok(builder.build()?)
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
