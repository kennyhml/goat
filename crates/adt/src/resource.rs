use std::fmt;

use url::Url;

use crate::{AdtUri, AdtUriError, ObjectRef};
const LINK_RESOLUTION_ORIGIN: &str = "https://adt.invalid";

/// A concrete link advertised by an ADT resource representation.
///
/// The original Atom metadata is retained alongside a resolved, validated
/// target. This lets callers use known relations through typed resource
/// references without discarding unknown relations or representation hints.
///
/// Handled by `IF_ATOM_TYPES=>link_s` on the backend.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[readonly::make]
pub struct AdtLink {
    /// The link exactly as advertised by SAP.
    pub href: String,

    /// The validated resource path produced by resolving `href`.
    pub target: AdtUri,

    /// Decoded query parameters in their advertised order.
    pub query: Vec<(String, String)>,

    /// The optional link fragment, without the leading `#`.
    pub fragment: Option<String>,

    /// The Atom relation identifying what the target means to its source.
    pub relation: Option<String>,

    /// The media type of the target representation, when advertised.
    pub media_type: Option<String>,

    /// The language of the target representation, when advertised.
    pub hreflang: Option<String>,

    /// A human-readable label for the link.
    pub title: Option<String>,

    /// The advertised target length.
    pub length: Option<String>,

    /// SAP's entity-tag extension for the target representation.
    pub etag: Option<String>,
}

impl AdtLink {
    pub(crate) fn from_href(
        base: &AdtUri,
        href: String,
        metadata: AdtLinkMetadata,
    ) -> Result<Self, AdtUriError> {
        let resolved = resolve_href(base, &href)?;
        Ok(Self {
            href,
            target: resolved.target,
            query: resolved.query,
            fragment: resolved.fragment,
            relation: metadata.relation,
            media_type: metadata.media_type,
            hreflang: metadata.hreflang,
            title: metadata.title,
            length: metadata.length,
            etag: metadata.etag,
        })
    }
}

pub(crate) struct AdtLinkMetadata {
    pub relation: Option<String>,
    pub media_type: Option<String>,
    pub hreflang: Option<String>,
    pub title: Option<String>,
    pub length: Option<String>,
    pub etag: Option<String>,
}

pub(crate) struct ResolvedHref {
    pub target: AdtUri,
    pub query: Vec<(String, String)>,
    pub fragment: Option<String>,
}

/// Resolves an href without assigning Atom link semantics to it.
pub(crate) fn resolve_href(base: &AdtUri, href: &str) -> Result<ResolvedHref, AdtUriError> {
    if href.is_empty() {
        return Err(AdtUriError::Empty);
    }
    if href.trim() != href || href.chars().any(char::is_control) || href.contains('\\') {
        return Err(AdtUriError::InvalidCharacters);
    }
    if href.starts_with("//") || Url::parse(href).is_ok() {
        return Err(AdtUriError::Absolute);
    }

    let base_url = Url::parse(&format!("{LINK_RESOLUTION_ORIGIN}{base}"))?;
    let resolved = if href.starts_with('/')
        || href.starts_with("./")
        || href.starts_with("../")
        || href.starts_with('?')
        || href.starts_with('#')
    {
        base_url.join(href)?
    } else {
        let mut directory = base_url;
        directory
            .path_segments_mut()
            .expect("an HTTP URL supports path segments")
            .push("");
        directory.join(href)?
    };

    Ok(ResolvedHref {
        target: AdtUri::parse(resolved.path())?,
        query: resolved
            .query_pairs()
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect(),
        fragment: resolved.fragment().map(str::to_owned),
    })
}

impl fmt::Display for AdtLink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.href)
    }
}

macro_rules! relation_ref {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, PartialEq)]
        #[readonly::make]
        pub struct $name {
            /// The validated related-resource URI.
            pub uri: AdtUri,

            /// Query parameters advertised as part of the relation.
            pub query: Vec<(String, String)>,

            /// The optional link fragment, without the leading `#`.
            pub fragment: Option<String>,

            /// The entity tag advertised for this resource, when present.
            pub etag: Option<String>,
        }

        impl $name {
            pub(crate) fn from_link(link: &AdtLink) -> Self {
                Self {
                    uri: link.target.clone(),
                    query: link.query.clone(),
                    fragment: link.fragment.clone(),
                    etag: link.etag.clone(),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.uri.fmt(formatter)
            }
        }
    };
}

relation_ref!(
    /// The rendered HTML representation of an object's source.
    HtmlSourceRef
);
relation_ref!(
    /// The version history advertised for a source resource.
    SourceVersionsRef
);
relation_ref!(
    /// The structural representation advertised for an ADT object.
    ObjectStructureRef
);
relation_ref!(
    /// The text-elements resource advertised for an ADT object.
    TextElementsRef
);
relation_ref!(
    /// The enhancement implementations associated with an ADT object.
    EnhancementImplementationsRef
);
relation_ref!(
    /// The enhancement options associated with an ADT object or source.
    EnhancementOptionsRef
);
relation_ref!(
    /// A link to another state, such as the active version, of an ADT object.
    ObjectStateRef
);
relation_ref!(
    /// The parser grammar advertised by an ABAP syntax configuration.
    ParserRef
);

/// A package reference embedded in an ADT object representation.
#[derive(Clone, Debug, Eq, PartialEq)]
#[readonly::make]
pub struct PackageRef {
    /// The package name.
    pub name: String,

    /// The repository object type, normally `DEVC/K`.
    pub object_type: String,

    /// The package's validated object reference.
    pub object: ObjectRef,
}

impl PackageRef {
    pub(crate) fn new(name: String, object_type: String, object: ObjectRef) -> Self {
        Self {
            name,
            object_type,
            object,
        }
    }
}

/// A validated source-code resource and its owning repository object.
///
/// A source URI alone does not establish which object lock authorizes an
/// update. `SourceRef` therefore retains both the source URI and its
/// [`ObjectRef`]. [`SourceRef::update`](crate::SourceRef::update) uses that
/// relationship to validate a [`LockHandle`](crate::LockHandle) at build time.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[readonly::make]
pub struct SourceRef {
    /// The object that owns this source resource.
    pub object: ObjectRef,

    /// The source resource URI.
    pub uri: AdtUri,

    /// Query parameters advertised as part of the source link.
    pub query: Vec<(String, String)>,

    /// The optional source fragment, without the leading `#`.
    pub fragment: Option<String>,

    /// The entity tag advertised for this source, when present.
    pub etag: Option<String>,
}

impl fmt::Display for SourceRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.uri.fmt(formatter)
    }
}

impl SourceRef {
    pub(crate) fn new(object: ObjectRef, uri: AdtUri) -> Self {
        Self {
            object,
            uri,
            query: Vec::new(),
            fragment: None,
            etag: None,
        }
    }

    pub(crate) fn from_link(object: ObjectRef, link: &AdtLink) -> Self {
        Self {
            object,
            uri: link.target.clone(),
            query: link.query.clone(),
            fragment: link.fragment.clone(),
            etag: link.etag.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_the_relative_link_forms_emitted_by_programs() {
        let program = AdtUri::parse("/sap/bc/adt/programs/programs/ZDEMO").unwrap();

        let source = resolve_href(&program, "source/main?version=active").unwrap();
        assert_eq!(
            source.target.as_str(),
            "/sap/bc/adt/programs/programs/ZDEMO/source/main"
        );
        assert_eq!(source.query, [("version".to_owned(), "active".to_owned())]);

        let structure = resolve_href(&program, "./ZDEMO/objectstructure?version=inactive").unwrap();
        assert_eq!(
            structure.target.as_str(),
            "/sap/bc/adt/programs/programs/ZDEMO/objectstructure"
        );
        assert_eq!(
            structure.query,
            [("version".to_owned(), "inactive".to_owned())]
        );

        let root_relative = resolve_href(
            &program,
            "/sap/bc/adt/textelements/programs/ZDEMO#selectionTexts",
        )
        .unwrap();
        assert_eq!(
            root_relative.target.as_str(),
            "/sap/bc/adt/textelements/programs/ZDEMO"
        );
        assert_eq!(root_relative.fragment.as_deref(), Some("selectionTexts"));
    }

    #[test]
    fn rejects_links_outside_the_sap_resource_namespace() {
        let program = AdtUri::parse("/sap/bc/adt/programs/programs/ZDEMO").unwrap();

        for href in [
            "https://attacker.example/sap/bc/adt/programs/ZDEMO",
            "//attacker.example/sap/bc/adt/programs/ZDEMO",
            "/sap/public/bc/icf/logoff",
            "../../../../../public/bc/icf/logoff",
        ] {
            assert!(resolve_href(&program, href).is_err(), "accepted {href}");
        }
    }
}
