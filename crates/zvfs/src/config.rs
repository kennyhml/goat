use zadt::{RepositoryFacet, RepositoryPreselection};

/// A caller-defined root entry in a virtual repository tree.
///
/// In Eclipse with ADT, that would be
/// ```text
/// A4H
/// └── Local Objects ($TMP)
/// └── Favorite Packages
/// └── Favorite Objects
/// └── System Libray
///```
/// They are static entry points into a vfs path with their own, customizable
/// [`FacetPolicy`], which controls the presentation of the objects, and
/// preselections, which control the displayed content.
#[derive(Clone, Debug)]
pub struct Mount {
    pub(crate) label: String,
    pub(crate) target: MountTarget,
    pub(crate) facet_policy: FacetPolicy,
}

#[derive(Clone, Debug)]
pub(crate) enum MountTarget {
    SystemLibrary,
    Package(String),
    Selection(Vec<RepositoryPreselection>),
}

impl Mount {
    /// Mounts the top-level package hierarchy.
    pub fn system_library(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            target: MountTarget::SystemLibrary,
            facet_policy: FacetPolicy::default(),
        }
    }

    /// Mounts one package using its technical package name as the label.
    pub fn package(package: impl Into<String>) -> Self {
        let package = package.into();
        Self::named_package(package.clone(), package)
    }

    /// Mounts one package with a caller-defined label.
    pub fn named_package(label: impl Into<String>, package: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            target: MountTarget::Package(package.into()),
            facet_policy: FacetPolicy::default(),
        }
    }

    /// Mounts an arbitrary RIS selection, such as favorites or local objects.
    pub fn selection(
        label: impl Into<String>,
        preselections: impl IntoIterator<Item = RepositoryPreselection>,
    ) -> Self {
        Self {
            label: label.into(),
            target: MountTarget::Selection(preselections.into_iter().collect()),
            facet_policy: FacetPolicy::default(),
        }
    }

    /// Overrides this mount's repository-object grouping policy.
    ///
    /// Mounts use [`FacetPolicy::default`] unless overridden.
    pub fn facet_policy(mut self, facet_policy: FacetPolicy) -> Self {
        self.facet_policy = facet_policy;
        self
    }

    /// Returns the display label used for this mount.
    pub fn label(&self) -> &str {
        &self.label
    }
}
/// Controls how matching repository objects are grouped into virtual folders.
///
/// RIS facets are metadata dimensions such as [`RepositoryFacet::OWNER`],
/// [`RepositoryFacet::GROUP`], and [`RepositoryFacet::TYPE`]. A mounts
/// preselections determine which objects match, its facet policy independently
/// determines the ordered folder levels used to present those objects.
///
/// **Expanding a facet folder adds its value to the current preselections before
/// the next policy level is evaluated.** A hierarchical facet can repeat at the
/// same level until RIS reports that it has no more same-facet children. After
/// the final configured level, the tree requests repository objects directly.
///
/// A [`FacetLevel::Always`] level is retained regardless of object count. An
/// [`FacetLevel::Adaptive`] level is retained when RIS reports at least its
/// configured minimum; otherwise only that level is skipped and evaluation
/// continues with the next one. Adaptive decisions are reevaluated on refresh,
/// so crossing a threshold can change the shape of that part of the tree.
///
/// Package hierarchy is handled separately from this policy. Package mounts
/// retain their package nodes and apply the policy to objects assigned directly
/// to each package.
///
/// The default policy groups by [`RepositoryFacet::GROUP`] and then
/// [`RepositoryFacet::TYPE`]. Use [`FacetPolicy::flat`] to disable configurable
/// facet-folder levels.
///
/// # Example
///
/// The following mount groups local objects by owner, broad repository group,
/// and object type:
///
/// ```
/// use zadt::{RepositoryFacet, RepositoryPreselection};
/// use zvfs::{FacetLevel, FacetPolicy, Mount};
///
/// let mount = Mount::selection(
///     "Local Objects ($TMP)",
///     [RepositoryPreselection::directly_assigned("$TMP")],
/// )
/// .facet_policy(FacetPolicy::new([
///     FacetLevel::always(RepositoryFacet::OWNER),
///     FacetLevel::always(RepositoryFacet::GROUP),
///     FacetLevel::always(RepositoryFacet::TYPE),
/// ]));
///
/// assert_eq!(mount.label(), "Local Objects ($TMP)");
/// ```
///
/// One resulting path can look like:
///
/// ```text
/// Local Objects ($TMP)            <- Mount
/// └── DEVELOPER                   <- OWNER facet
///     └── Source Code Library     <- GROUP facet
///         └── Classes             <- TYPE  facet
///             └── ZCL_MY_CLASS
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FacetPolicy {
    levels: Vec<FacetLevel>,
}

impl FacetPolicy {
    /// Creates a policy whose levels are evaluated in iteration order.
    pub fn new(levels: impl IntoIterator<Item = FacetLevel>) -> Self {
        Self {
            levels: levels.into_iter().collect(),
        }
    }

    /// Returns objects without configurable facet-folder levels.
    ///
    /// Package hierarchy remains visible because it is independent of the
    /// repository-object facet policy.
    pub fn flat() -> Self {
        Self::new([])
    }

    /// Creates an ordered policy in which every facet uses
    /// [`FacetLevel::Always`].
    pub fn grouped<I, F>(facets: I) -> Self
    where
        I: IntoIterator<Item = F>,
        F: Into<RepositoryFacet>,
    {
        Self::new(facets.into_iter().map(FacetLevel::always))
    }

    /// Returns the facet levels in traversal order.
    pub fn levels(&self) -> &[FacetLevel] {
        &self.levels
    }
}

impl Default for FacetPolicy {
    /// Groups repository objects by broad group and then concrete object type.
    fn default() -> Self {
        Self::grouped([RepositoryFacet::GROUP, RepositoryFacet::TYPE])
    }
}

/// One repository-object grouping level in a mount's ordered [`FacetPolicy`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FacetLevel {
    /// Retain this facet as a virtual-folder level regardless of object count.
    Always { facet: RepositoryFacet },

    /// Retain this facet only when the current selection contains at least the
    /// configured number of objects.
    Adaptive {
        facet: RepositoryFacet,
        minimum_objects: u32,
    },
}

impl FacetLevel {
    /// Creates a grouping level that is retained regardless of object count.
    pub fn always(facet: impl Into<RepositoryFacet>) -> Self {
        Self::Always {
            facet: facet.into(),
        }
    }

    /// Creates a grouping level retained when the current object count is
    /// greater than or equal to `minimum_objects`.
    pub fn adaptive(facet: impl Into<RepositoryFacet>, minimum_objects: u32) -> Self {
        Self::Adaptive {
            facet: facet.into(),
            minimum_objects,
        }
    }

    /// Returns the RIS facet represented by this level.
    pub fn facet(&self) -> &RepositoryFacet {
        match self {
            Self::Always { facet } | Self::Adaptive { facet, .. } => facet,
        }
    }

    /// Returns whether this level should be retained for an object count.
    pub(crate) fn retains(&self, object_count: u32) -> bool {
        match self {
            Self::Always { .. } => true,
            Self::Adaptive {
                minimum_objects, ..
            } => object_count >= *minimum_objects,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_always_groups_by_group_then_type() {
        let policy = FacetPolicy::default();

        assert_eq!(
            policy.levels(),
            [
                FacetLevel::always(RepositoryFacet::GROUP),
                FacetLevel::always(RepositoryFacet::TYPE),
            ]
        );
    }

    #[test]
    fn adaptive_thresholds_apply_only_to_their_level() {
        let policy = FacetPolicy::new([
            FacetLevel::always(RepositoryFacet::GROUP),
            FacetLevel::adaptive(RepositoryFacet::TYPE, 10),
        ]);

        assert!(policy.levels()[0].retains(0));
        assert!(!policy.levels()[1].retains(9));
        assert!(policy.levels()[1].retains(10));
    }

    #[test]
    fn mount_policy_overrides_do_not_change_other_mounts() {
        let flat = Mount::system_library("Flat").facet_policy(FacetPolicy::flat());
        let default = Mount::system_library("Default");

        assert!(flat.facet_policy.levels().is_empty());
        assert_eq!(default.facet_policy, FacetPolicy::default());
    }
}
