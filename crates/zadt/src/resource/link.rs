use std::fmt;

use serde::Deserialize;
use url::Url;

use crate::{AdtUri, AdtUriError, vocabulary::Relation};

const LINK_RESOLUTION_ORIGIN: &str = "https://adt.invalid";

/// A resolved link that points to another resource.
///
/// The original Atom metadata is retained alongside a resolved, validated
/// target. Known relations can also be converted into typed resource
/// references.
///
/// For example, an advertised URI may be relative `href=source/main`,
/// an absolute path `href=sap/bc/adt/textelements/programs/zprog`, or
/// contain query parameters, such as `?version=Active`. All of this
/// information validated and resolved against the base resource.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AdtLink {
    /// The raw link exactly as advertised by ADT.
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

    /// Entity-Ta for the target representation.
    pub etag: Option<String>,
}

impl fmt::Display for AdtLink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.href)
    }
}

/// A raw, unverified `atom:link` advertised by a resource based on
/// a HATEOAS API design.
///
/// This type is an implementation detail and essentially a stepping
/// stone for the more meaningful [`AdtLink].
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq)]
pub(crate) struct AdvertisedLink {
    #[serde(rename = "@href")]
    pub href: String,

    #[serde(rename = "@rel")]
    pub relation: Option<String>,

    #[serde(rename = "@type")]
    pub media_type: Option<String>,

    #[serde(rename = "@hreflang")]
    pub hreflang: Option<String>,

    #[serde(rename = "@title")]
    pub title: Option<String>,

    #[serde(rename = "@length")]
    pub length: Option<String>,

    #[serde(rename = "@etag")]
    pub etag: Option<String>,
}

impl AdvertisedLink {
    pub(super) fn resolve(&self, base: &AdtUri) -> Result<AdtLink, AdtLinkError> {
        let resolved = resolve_href(base, &self.href).map_err(|source| AdtLinkError {
            href: self.href.clone(),
            source,
        })?;
        Ok(AdtLink {
            href: self.href.clone(),
            target: resolved.target,
            query: resolved.query,
            fragment: resolved.fragment,
            relation: self.relation.clone(),
            media_type: self.media_type.clone(),
            hreflang: self.hreflang.clone(),
            title: self.title.clone(),
            length: self.length.clone(),
            etag: self.etag.clone(),
        })
    }

    pub(super) fn matches(&self, relation: Relation, media_type: Option<&str>) -> bool {
        self.relation.as_deref().and_then(Relation::from_uri) == Some(relation)
            && media_type.is_none_or(|expected| {
                self.media_type.as_deref().is_some_and(|actual| {
                    actual
                        .split(';')
                        .next()
                        .is_some_and(|value| value.trim().eq_ignore_ascii_case(expected))
                })
            })
    }
}

/// An advertised ADT link whose target could not be resolved safely.
#[derive(Debug, thiserror::Error)]
#[error("ADT link `{href}` could not be resolved: {source}")]
pub struct AdtLinkError {
    href: String,
    source: AdtUriError,
}

impl AdtLinkError {
    /// Returns the unresolved href exactly as advertised by SAP.
    pub fn href(&self) -> &str {
        &self.href
    }

    pub(crate) fn into_parts(self) -> (String, AdtUriError) {
        (self.href, self.source)
    }
}

/// Internal implementation of resolving the url advertised in an
/// href attribute of an `atom:link`.
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
