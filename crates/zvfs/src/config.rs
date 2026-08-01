use zadt::{RepositoryFacet, RepositoryPreselection};

/// Controls the virtual folders inserted between a selection and its objects.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FacetPolicy {
    /// Return repository objects without virtual grouping folders.
    Flat,

    /// Apply every configured facet in order.
    Grouped { facets: Vec<RepositoryFacet> },

    /// Apply the next facet only when the current layer contains enough objects.
    Adaptive {
        facets: Vec<RepositoryFacet>,
        minimum_objects: u32,
    },
}

impl FacetPolicy {
    /// Creates an always-grouped policy from an ordered facet chain.
    pub fn grouped(facets: impl IntoIterator<Item = RepositoryFacet>) -> Self {
        Self::Grouped {
            facets: facets.into_iter().collect(),
        }
    }

    /// Creates a count-sensitive policy from an ordered facet chain.
    pub fn adaptive(
        minimum_objects: u32,
        facets: impl IntoIterator<Item = RepositoryFacet>,
    ) -> Self {
        Self::Adaptive {
            facets: facets.into_iter().collect(),
            minimum_objects,
        }
    }

    pub(crate) fn facets(&self) -> &[RepositoryFacet] {
        match self {
            Self::Flat => &[],
            Self::Grouped { facets } | Self::Adaptive { facets, .. } => facets,
        }
    }

    pub(crate) fn minimum_objects(&self) -> Option<u32> {
        match self {
            Self::Adaptive {
                minimum_objects, ..
            } => Some(*minimum_objects),
            Self::Flat | Self::Grouped { .. } => None,
        }
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
        }
    }

    /// Returns the display label used for this mount.
    pub fn label(&self) -> &str {
        &self.label
    }
}
