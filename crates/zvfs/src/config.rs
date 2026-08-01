use zadt::{RepositoryFacet, RepositoryPreselection};

/// One virtual-folder level in a mount's ordered facet policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FacetLevel {
    /// Always retain this facet as a directory level.
    Always { facet: RepositoryFacet },

    /// Retain this facet only when the current selection contains enough objects.
    Adaptive {
        facet: RepositoryFacet,
        minimum_objects: u32,
    },
}

impl FacetLevel {
    /// Creates a facet level that is always retained.
    pub fn always(facet: impl Into<RepositoryFacet>) -> Self {
        Self::Always {
            facet: facet.into(),
        }
    }

    /// Creates a facet level retained at or above `minimum_objects`.
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

/// Controls the virtual folders inserted between one mount and its objects.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FacetPolicy {
    levels: Vec<FacetLevel>,
}

impl FacetPolicy {
    /// Creates an ordered policy from independently configured facet levels.
    pub fn new(levels: impl IntoIterator<Item = FacetLevel>) -> Self {
        Self {
            levels: levels.into_iter().collect(),
        }
    }

    /// Returns repository objects without virtual grouping folders.
    pub fn flat() -> Self {
        Self::new([])
    }

    /// Creates an always-grouped policy from an ordered facet chain.
    pub fn grouped<I, F>(facets: I) -> Self
    where
        I: IntoIterator<Item = F>,
        F: Into<RepositoryFacet>,
    {
        Self::new(facets.into_iter().map(FacetLevel::always))
    }

    /// Returns the ordered facet levels.
    pub fn levels(&self) -> &[FacetLevel] {
        &self.levels
    }
}

impl Default for FacetPolicy {
    fn default() -> Self {
        Self::grouped([RepositoryFacet::GROUP, RepositoryFacet::TYPE])
    }
}

/// A caller-defined root entry in a repository VFS.
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

    /// Sets the ordered virtual-folder policy for this mount.
    pub fn facet_policy(mut self, facet_policy: FacetPolicy) -> Self {
        self.facet_policy = facet_policy;
        self
    }

    /// Returns the display label used for this mount.
    pub fn label(&self) -> &str {
        &self.label
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
