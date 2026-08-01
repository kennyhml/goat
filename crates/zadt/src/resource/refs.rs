use std::{fmt, marker::PhantomData};

use crate::{
    AdtUri, ObjectRef,
    vocabulary::{Relation, media_type},
};

use super::AdtLink;

pub(crate) trait FromAdtLink: Sized {
    const RELATION: Relation;
    const MEDIA_TYPE: Option<&'static str> = None;

    fn from_adt_link(owner: &ObjectRef, link: &AdtLink) -> OwnedResourceRef<Self> {
        OwnedResourceRef::from_link(owner.clone(), link)
    }
}

/// A typed related-resource location and the object that advertised it.
///
/// The marker identifies the relation represented by the reference. Named
/// aliases such as [`TextElementsRef`] provide the public relation-specific API.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct OwnedResourceRef<T> {
    /// The repository object that owns this related resource.
    pub object: ObjectRef,

    /// The validated related-resource URI.
    pub uri: AdtUri,

    /// Query parameters advertised as part of the relation.
    pub query: Vec<(String, String)>,

    /// The optional link fragment, without the leading `#`.
    pub fragment: Option<String>,

    /// The entity tag advertised for this resource, when present.
    pub etag: Option<String>,

    marker: PhantomData<fn() -> T>,
}

impl<T> OwnedResourceRef<T> {
    pub(crate) fn new(object: ObjectRef, uri: AdtUri) -> Self {
        Self {
            object,
            uri,
            query: Vec::new(),
            fragment: None,
            etag: None,
            marker: PhantomData,
        }
    }

    fn from_link(object: ObjectRef, link: &AdtLink) -> Self {
        Self {
            query: link.query.clone(),
            fragment: link.fragment.clone(),
            etag: link.etag.clone(),
            ..Self::new(object, link.target.clone())
        }
    }
}

impl<T> fmt::Display for OwnedResourceRef<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.uri.fmt(formatter)
    }
}

mod kind {
    use super::{FromAdtLink, Relation, media_type};

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct HtmlSource;

    impl FromAdtLink for HtmlSource {
        const RELATION: Relation = Relation::Source;
        const MEDIA_TYPE: Option<&'static str> = Some(media_type::HTML);
    }

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct SourceVersions;

    impl FromAdtLink for SourceVersions {
        const RELATION: Relation = Relation::Versions;
    }

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct ObjectStructure;

    impl FromAdtLink for ObjectStructure {
        const RELATION: Relation = Relation::ObjectStructure;
    }

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct TextElements;

    impl FromAdtLink for TextElements {
        const RELATION: Relation = Relation::TextElements;
    }

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct EnhancementImplementations;

    impl FromAdtLink for EnhancementImplementations {
        const RELATION: Relation = Relation::EnhancementImplementations;
    }

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct ObjectEnhancementOptions;

    impl FromAdtLink for ObjectEnhancementOptions {
        const RELATION: Relation = Relation::ObjectEnhancementOptions;
    }

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct SourceEnhancementOptions;

    impl FromAdtLink for SourceEnhancementOptions {
        const RELATION: Relation = Relation::SourceEnhancementOptions;
    }

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct ObjectState;

    impl FromAdtLink for ObjectState {
        const RELATION: Relation = Relation::ObjectStates;
    }

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct Parser;

    impl FromAdtLink for Parser {
        const RELATION: Relation = Relation::Parser;
    }

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct Source;

    impl FromAdtLink for Source {
        const RELATION: Relation = Relation::Source;
        const MEDIA_TYPE: Option<&'static str> = Some(media_type::SOURCE);
    }
}

/// The rendered HTML representation of an object's source.
pub type HtmlSourceRef = OwnedResourceRef<kind::HtmlSource>;

/// The version history advertised for a source resource.
pub type SourceVersionsRef = OwnedResourceRef<kind::SourceVersions>;

/// The structural representation advertised for an ADT object.
pub type ObjectStructureRef = OwnedResourceRef<kind::ObjectStructure>;

/// The text-elements resource advertised for an ADT object.
pub type TextElementsRef = OwnedResourceRef<kind::TextElements>;

/// The enhancement implementations associated with an ADT object.
pub type EnhancementImplementationsRef = OwnedResourceRef<kind::EnhancementImplementations>;

/// The enhancement options associated with an ADT object.
pub type ObjectEnhancementOptionsRef = OwnedResourceRef<kind::ObjectEnhancementOptions>;

/// The enhancement options associated with an ADT source.
pub type SourceEnhancementOptionsRef = OwnedResourceRef<kind::SourceEnhancementOptions>;

/// A link to another state, such as the active version, of an ADT object.
pub type ObjectStateRef = OwnedResourceRef<kind::ObjectState>;

/// The parser grammar advertised by an ABAP syntax configuration.
pub type ParserRef = OwnedResourceRef<kind::Parser>;

/// A validated source-code resource and its owning repository object.
///
/// A source URI alone does not establish which object lock authorizes an
/// update. `SourceRef` therefore retains both the source URI and its
/// [`ObjectRef`]. [`SourceRef::update`](crate::SourceRef::update) uses that
/// relationship to validate a [`LockHandle`](crate::LockHandle) at build time.
pub type SourceRef = OwnedResourceRef<kind::Source>;
