use serde::Deserialize;
use url::Url;

use crate::{AdtUri, DiscoveryError};

pub(crate) fn parse_capabilities(body: &[u8]) -> Result<Capabilities, DiscoveryError> {
    let raw: RawService = serde_xml_rs::from_reader(body)?;
    Capabilities::try_from(raw)
}

/// Capabilities advertised by an ADT discovery document.
///
/// A capability document consists of one or more [`Workspace`] values, each
/// containing related [`Collection`] values. Use [`Capabilities::collection`]
/// with a category scheme and term when selecting a protocol capability.
#[derive(Debug)]
pub struct Capabilities {
    workspaces: Vec<Workspace>,
}

impl Capabilities {
    /// Returns all workspaces in document order.
    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    /// Finds the first collection advertising the category `scheme` and `term`.
    ///
    /// Category pairs are preferable to workspace or collection titles for
    /// capability lookup because titles are display text and can be localized.
    pub fn collection(&self, scheme: &str, term: &str) -> Option<&Collection> {
        self.workspaces
            .iter()
            .flat_map(Workspace::collections)
            .find(|collection| {
                collection
                    .categories
                    .iter()
                    .any(|category| category.scheme == scheme && category.term == term)
            })
    }
}

/// A named group of related ADT collections.
#[derive(Debug)]
pub struct Workspace {
    title: String,
    collections: Vec<Collection>,
}

impl Workspace {
    /// Returns the workspace's display title.
    ///
    /// The server can localize this value; do not use it as a protocol key.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the collections advertised in this workspace.
    pub fn collections(&self) -> &[Collection] {
        &self.collections
    }
}

/// An ADT resource collection advertised by a discovery document.
#[derive(Debug)]
pub struct Collection {
    href: String,
    target: AdtUri,
    title: Option<String>,
    accepted_media_types: Vec<String>,
    categories: Vec<Category>,
    template_links: Vec<TemplateLink>,
}

impl Collection {
    /// Returns the collection `href` exactly as advertised by the server.
    ///
    /// Most collections use a root-relative URI, but AtomPub also permits an
    /// absolute URL. Use [`Collection::target`] when constructing an operation
    /// for the configured transport destination.
    pub fn href(&self) -> &str {
        &self.href
    }

    /// Returns the validated, destination-relative target for the collection.
    ///
    /// For an absolute advertised `href`, this preserves the path while
    /// discarding its authority. Requests therefore remain bound to the
    /// transport's configured SAP destination.
    pub fn target(&self) -> &AdtUri {
        &self.target
    }

    /// Returns the collection's optional display title.
    ///
    /// SAP can omit or localize this value; use [`Collection::categories`] to
    /// identify a capability programmatically.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Returns the representation media types accepted by the collection.
    pub fn accepted_media_types(&self) -> &[String] {
        &self.accepted_media_types
    }

    /// Returns the protocol categories advertised by the collection.
    pub fn categories(&self) -> &[Category] {
        &self.categories
    }

    /// Returns the collections parameterized links to related resources.
    pub fn template_links(&self) -> &[TemplateLink] {
        &self.template_links
    }
}

/// A stable category identity advertised for a collection.
///
/// ADT identifies a category by the combination of its [`scheme`](Self::scheme)
/// and [`term`](Self::term).
#[derive(Debug)]
pub struct Category {
    term: String,
    scheme: String,
}

impl Category {
    /// Returns the category term within its scheme.
    pub fn term(&self) -> &str {
        &self.term
    }

    /// Returns the category scheme URI.
    pub fn scheme(&self) -> &str {
        &self.scheme
    }
}

/// A parameterized link from a collection to a related ADT resource.
#[derive(Debug)]
pub struct TemplateLink {
    title: Option<String>,
    relation: String,
    template: String,
    media_type: Option<String>,
}

impl TemplateLink {
    /// Returns the optional display title of the link.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Returns the relation URI describing the link's semantics.
    pub fn relation(&self) -> &str {
        &self.relation
    }

    /// Returns the URI template supplied by the server.
    pub fn template(&self) -> &str {
        &self.template
    }

    /// Returns the optional media type associated with the target resource.
    pub fn media_type(&self) -> Option<&str> {
        self.media_type.as_deref()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename = "app:service")]
struct RawService {
    #[serde(rename = "app:workspace", default)]
    workspaces: Vec<RawWorkspace>,
}

#[derive(Debug, Deserialize)]
struct RawWorkspace {
    #[serde(rename = "atom:title")]
    title: String,

    #[serde(rename = "app:collection", default)]
    collections: Vec<RawCollection>,
}

#[derive(Debug, Deserialize)]
struct RawCollection {
    #[serde(rename = "@href")]
    href: Option<String>,

    #[serde(rename = "atom:title")]
    title: Option<String>,

    #[serde(rename = "app:accept", default)]
    accepted_media_types: Vec<String>,

    #[serde(rename = "atom:category", default)]
    categories: Vec<RawCategory>,

    #[serde(rename = "adtcomp:templateLinks", default)]
    template_links: RawTemplateLinks,
}

#[derive(Debug, Deserialize)]
struct RawCategory {
    #[serde(rename = "@term")]
    term: String,

    #[serde(rename = "@scheme")]
    scheme: String,
}

#[derive(Debug, Default, Deserialize)]
struct RawTemplateLinks {
    #[serde(rename = "adtcomp:templateLink", default)]
    links: Vec<RawTemplateLink>,
}

#[derive(Debug, Deserialize)]
struct RawTemplateLink {
    #[serde(rename = "@title")]
    title: Option<String>,

    #[serde(rename = "@rel")]
    relation: String,

    #[serde(rename = "@template")]
    template: String,

    #[serde(rename = "@type")]
    media_type: Option<String>,
}

impl TryFrom<RawService> for Capabilities {
    type Error = DiscoveryError;

    fn try_from(raw: RawService) -> Result<Self, Self::Error> {
        let workspaces = raw
            .workspaces
            .into_iter()
            .map(|workspace| {
                let collections = workspace
                    .collections
                    .into_iter()
                    .map(Collection::try_from)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Workspace {
                    title: workspace.title,
                    collections,
                })
            })
            .collect::<Result<Vec<_>, DiscoveryError>>()?;

        Ok(Self { workspaces })
    }
}

impl TryFrom<RawCollection> for Collection {
    type Error = DiscoveryError;

    fn try_from(collection: RawCollection) -> Result<Self, Self::Error> {
        let error_title = collection
            .title
            .as_deref()
            .unwrap_or("<untitled>")
            .to_owned();
        let href = collection
            .href
            .ok_or_else(|| DiscoveryError::MissingCollectionHref {
                title: error_title.clone(),
            })?;
        let target =
            collection_target(&href).map_err(|source| DiscoveryError::InvalidCollectionHref {
                title: error_title,
                href: href.clone(),
                source,
            })?;

        Ok(Self {
            href,
            target,
            title: collection.title,
            accepted_media_types: collection.accepted_media_types,
            categories: collection
                .categories
                .into_iter()
                .map(|category| Category {
                    term: category.term,
                    scheme: category.scheme,
                })
                .collect(),
            template_links: collection
                .template_links
                .links
                .into_iter()
                .map(|link| TemplateLink {
                    title: link.title,
                    relation: link.relation,
                    template: link.template,
                    media_type: link.media_type,
                })
                .collect(),
        })
    }
}

fn collection_target(href: &str) -> Result<AdtUri, crate::AdtUriError> {
    match AdtUri::parse(href) {
        Ok(target) => Ok(target),
        Err(crate::AdtUriError::Absolute) if !href.starts_with("//") => {
            let url = Url::parse(href)?;
            if !matches!(url.scheme(), "http" | "https")
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(crate::AdtUriError::Absolute);
            }
            if url.query().is_some() || url.fragment().is_some() {
                return Err(crate::AdtUriError::QueryOrFragment);
            }
            AdtUri::parse(url.path())
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vocabulary::PROGRAMS;

    const DISCOVERY_XML: &[u8] = include_bytes!("../tests/fixtures/discovery.xml");
    const INVALID_DISCOVERY_XML: &[u8] = include_bytes!("../tests/fixtures/invalid-discovery.xml");

    #[test]
    fn parses_discovery_capabilities() {
        let capabilities = parse_capabilities(DISCOVERY_XML).unwrap();
        let collection = capabilities
            .collection(PROGRAMS.scheme, PROGRAMS.term)
            .unwrap();

        assert_eq!(capabilities.workspaces()[0].title(), "Programme");
        assert_eq!(collection.href(), "/sap/bc/adt/programs/programs");
        assert_eq!(collection.target().as_str(), collection.href());
        assert_eq!(
            collection.accepted_media_types(),
            [
                "application/vnd.sap.adt.programs.programs.v2+xml",
                "application/vnd.sap.adt.programs.programs.v3+xml",
            ]
        );
        assert_eq!(
            collection.template_links()[0].relation(),
            "http://www.sap.com/adt/categories/programs/valuehelp/application"
        );
    }

    #[test]
    fn rejects_invalid_collection_targets() {
        let error = parse_capabilities(INVALID_DISCOVERY_XML).unwrap_err();

        assert!(matches!(
            error,
            DiscoveryError::InvalidCollectionHref { .. }
        ));
    }

    #[test]
    fn rejects_malformed_discovery_xml() {
        let error = parse_capabilities(b"<app:service>").unwrap_err();

        assert!(matches!(error, DiscoveryError::Xml(_)));
    }

    #[test]
    fn accepts_a_collection_without_a_title() {
        let capabilities = parse_capabilities(
            br#"<app:service xmlns:app="http://www.w3.org/2007/app"
                    xmlns:atom="http://www.w3.org/2005/Atom">
                    <app:workspace>
                        <atom:title>Services</atom:title>
                        <app:collection href="/sap/bc/adt/services" />
                    </app:workspace>
                </app:service>"#,
        )
        .unwrap();

        assert_eq!(capabilities.workspaces()[0].collections()[0].title(), None);
    }

    #[test]
    fn retains_an_absolute_href_with_a_destination_relative_target() {
        let capabilities = parse_capabilities(
            br#"<app:service xmlns:app="http://www.w3.org/2007/app"
                    xmlns:atom="http://www.w3.org/2005/Atom">
                    <app:workspace>
                        <atom:title>Transport</atom:title>
                        <app:collection href="https://sap.example.test:44300/sap/bc/adt">
                            <atom:title>ADT endpoint</atom:title>
                        </app:collection>
                    </app:workspace>
                </app:service>"#,
        )
        .unwrap();
        let collection = &capabilities.workspaces()[0].collections()[0];

        assert_eq!(
            collection.href(),
            "https://sap.example.test:44300/sap/bc/adt"
        );
        assert_eq!(collection.target().as_str(), "/sap/bc/adt");
    }
}
