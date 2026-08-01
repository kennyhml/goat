use derive_builder::Builder;
use http::{Method, StatusCode};

use crate::{
    AdtRequest, AdtResponse, AdtUri, CategoryId, Client, ObjectRef, Operation, OperationError,
    Ready, RepositoryContent, RepositoryFacet, RepositoryFacets, RepositoryObjectProperties,
    RepositoryPreselection, ResponseError, Stateless, models::RepositoryContentRequest,
    target::CollectionTarget, vocabulary::media_type,
};

/// Selects whether a repository-content query expands or only counts matches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryContentOperation {
    Expand,
    Count,
}

impl RepositoryContentOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Expand => "expand",
            Self::Count => "count",
        }
    }
}

/// Fetches one virtual-folder hierarchy layer from the repository information system.
///
/// RIS does not recursively expand the complete hierarchy. Callers traverse a
/// tree by adding the returned folder values as preselections in subsequent
/// queries.
#[derive(Builder, Clone, Debug)]
#[builder(pattern = "owned", setter(into), default)]
pub struct RepositoryContentQuery {
    search_pattern: String,

    #[builder(setter(each(name = "preselection")))]
    preselections: Vec<RepositoryPreselection>,

    #[builder(setter(each(name = "facet")))]
    facets: Vec<RepositoryFacet>,

    #[builder(setter(strip_option))]
    operation: Option<RepositoryContentOperation>,

    #[builder(setter(strip_option))]
    ignore_short_descriptions: Option<bool>,

    #[builder(setter(strip_option))]
    with_versions: Option<bool>,
}

impl Default for RepositoryContentQuery {
    fn default() -> Self {
        Self {
            search_pattern: "*".to_owned(),
            preselections: Vec::new(),
            facets: Vec::new(),
            operation: None,
            ignore_short_descriptions: None,
            with_versions: None,
        }
    }
}

impl RepositoryContentQuery {
    const TARGET: CollectionTarget = CollectionTarget::new(CategoryId {
        scheme: "http://www.sap.com/adt/categories/repository/virtualfolders",
        term: "contents",
    });

    /// Creates a query matching all repository object names.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a builder for a configurable repository-content query.
    pub fn builder() -> RepositoryContentQueryBuilder {
        RepositoryContentQueryBuilder::default()
    }
}

impl Operation<Ready> for RepositoryContentQuery {
    type Response = RepositoryContent;
    type Kind = Stateless;

    fn request(&self, client: &Client<Ready>) -> Result<AdtRequest, OperationError> {
        let body =
            RepositoryContentRequest::new(&self.search_pattern, &self.preselections, &self.facets)
                .serialize()?;
        let mut request = Self::TARGET.request(client, Method::POST)?;
        if let Some(ignore) = self.ignore_short_descriptions {
            request.push_query("ignoreShortDescriptions", ignore.to_string());
        }
        if let Some(with_versions) = self.with_versions {
            request.push_query("withVersions", with_versions.to_string());
        }
        if let Some(operation) = self.operation {
            request.push_query("operation", operation.as_str());
        }
        request.set_content_type(media_type::REPOSITORY_CONTENT_REQUEST);
        request.set_body(body);
        Ok(request)
    }

    fn decode(
        &self,
        response: AdtResponse,
        request_target: &AdtUri,
    ) -> Result<Self::Response, ResponseError> {
        ensure_ok(&response)?;
        RepositoryContent::parse(response.body(), request_target).map_err(Into::into)
    }
}

/// Fetches the facets supported by the repository information system.
#[derive(Clone, Copy, Debug, Default)]
pub struct RepositoryFacetsQuery;

impl RepositoryFacetsQuery {
    const TARGET: CollectionTarget = CollectionTarget::new(CategoryId {
        scheme: "http://www.sap.com/adt/categories/repository/virtualfolders",
        term: "facets",
    });
}

impl Operation<Ready> for RepositoryFacetsQuery {
    type Response = RepositoryFacets;
    type Kind = Stateless;

    fn request(&self, client: &Client<Ready>) -> Result<AdtRequest, OperationError> {
        Self::TARGET.request(client, Method::GET)
    }

    fn decode(
        &self,
        response: AdtResponse,
        _request_target: &AdtUri,
    ) -> Result<Self::Response, ResponseError> {
        ensure_ok(&response)?;
        RepositoryFacets::parse(response.body()).map_err(Into::into)
    }
}

/// Fetches uniform RIS properties for an arbitrary repository object.
#[derive(Builder, Clone, Debug)]
#[builder(pattern = "owned", setter(into))]
pub struct RepositoryObjectPropertiesQuery {
    object_uri: AdtUri,

    #[builder(default, setter(each(name = "include_facet")))]
    facets: Vec<RepositoryFacet>,
}

impl RepositoryObjectPropertiesQuery {
    const TARGET: CollectionTarget = CollectionTarget::new(CategoryId {
        scheme: "http://www.sap.com/adt/categories/repository",
        term: "objectProperties",
    });

    /// Creates a property query for a validated object reference.
    pub fn new<T>(object: &ObjectRef<T>) -> Self {
        Self::for_uri(object.uri().clone())
    }

    /// Creates a property query from a validated ADT object URI.
    pub fn for_uri(object_uri: AdtUri) -> Self {
        Self {
            object_uri,
            facets: Vec::new(),
        }
    }

    /// Creates a builder initialized for a validated object reference.
    pub fn builder<T>(object: &ObjectRef<T>) -> RepositoryObjectPropertiesQueryBuilder {
        RepositoryObjectPropertiesQueryBuilder::default().object_uri(object.uri().clone())
    }

    /// Creates a builder initialized with a validated ADT object URI.
    pub fn builder_for_uri(object_uri: AdtUri) -> RepositoryObjectPropertiesQueryBuilder {
        RepositoryObjectPropertiesQueryBuilder::default().object_uri(object_uri)
    }
}

impl Operation<Ready> for RepositoryObjectPropertiesQuery {
    type Response = RepositoryObjectProperties;
    type Kind = Stateless;

    fn request(&self, client: &Client<Ready>) -> Result<AdtRequest, OperationError> {
        let mut request = Self::TARGET.request(client, Method::GET)?;
        request.push_query("uri", self.object_uri.as_str());
        for facet in &self.facets {
            request.push_query("facet", facet.as_str());
        }
        request.set_accept(media_type::REPOSITORY_OBJECT_PROPERTIES);
        Ok(request)
    }

    fn decode(
        &self,
        response: AdtResponse,
        _request_target: &AdtUri,
    ) -> Result<Self::Response, ResponseError> {
        ensure_ok(&response)?;
        RepositoryObjectProperties::parse(response.body(), &self.object_uri).map_err(Into::into)
    }
}

impl crate::RepositoryObjectEntry {
    /// Creates a uniform RIS property query for this listed object.
    pub fn properties(&self) -> RepositoryObjectPropertiesQuery {
        RepositoryObjectPropertiesQuery::new(&self.reference)
    }

    /// Creates a configurable uniform RIS property query for this listed object.
    pub fn properties_builder(&self) -> RepositoryObjectPropertiesQueryBuilder {
        RepositoryObjectPropertiesQuery::builder(&self.reference)
    }
}

fn ensure_ok(response: &AdtResponse) -> Result<(), ResponseError> {
    if response.status() == StatusCode::OK {
        Ok(())
    } else {
        Err(ResponseError::UnexpectedStatus {
            status: response.status(),
            body: String::from_utf8_lossy(response.body()).into_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use http::{HeaderMap, header};

    use super::*;
    use crate::{CompatibilityError, Program, Transport, TransportError};

    const CONTENT_XML: &[u8] = include_bytes!("../../tests/fixtures/repository-content.xml");
    const OBJECT_PROPERTIES_XML: &[u8] =
        include_bytes!("../../tests/fixtures/repository-object-properties.xml");
    const REPOSITORY_DISCOVERY_XML: &[u8] = br#"
        <app:service xmlns:app="http://www.w3.org/2007/app"
                xmlns:atom="http://www.w3.org/2005/Atom">
            <app:workspace>
                <atom:title>Repository</atom:title>
                <app:collection href="/sap/bc/adt/advertised/repository/contents">
                    <atom:category term="contents"
                        scheme="http://www.sap.com/adt/categories/repository/virtualfolders" />
                </app:collection>
                <app:collection href="/sap/bc/adt/advertised/repository/facets">
                    <atom:category term="facets"
                        scheme="http://www.sap.com/adt/categories/repository/virtualfolders" />
                </app:collection>
                <app:collection href="/sap/bc/adt/advertised/repository/object-properties">
                    <atom:category term="objectProperties"
                        scheme="http://www.sap.com/adt/categories/repository" />
                </app:collection>
            </app:workspace>
        </app:service>
    "#;

    struct UnusedTransport;

    #[async_trait]
    impl Transport for UnusedTransport {
        async fn send(&self, _request: AdtRequest) -> Result<AdtResponse, TransportError> {
            unreachable!("request construction tests do not send requests")
        }
    }

    fn ready_client(xml: &[u8]) -> Client<Ready> {
        Client::new(UnusedTransport)
            .with_capabilities(crate::models::parse_capabilities(xml).unwrap())
    }

    #[test]
    fn repository_content_request_matches_the_ris_contract() {
        let client = ready_client(REPOSITORY_DISCOVERY_XML);
        let query = RepositoryContentQuery::builder()
            .search_pattern("Z*")
            .preselection(
                RepositoryPreselection::new(RepositoryFacet::PACKAGE, "$TMP").exclude("UI5/STRU"),
            )
            .facet(RepositoryFacet::GROUP)
            .operation(RepositoryContentOperation::Expand)
            .ignore_short_descriptions(true)
            .with_versions(false)
            .build()
            .unwrap();

        let request = query.request(&client).unwrap();
        let body = std::str::from_utf8(request.body()).unwrap();

        assert_eq!(request.method(), Method::POST);
        assert_eq!(
            request.target().as_str(),
            "/sap/bc/adt/advertised/repository/contents"
        );
        assert_eq!(
            request.query(),
            [
                ("ignoreShortDescriptions".to_owned(), "true".to_owned()),
                ("withVersions".to_owned(), "false".to_owned()),
                ("operation".to_owned(), "expand".to_owned()),
            ]
        );
        assert_eq!(
            request.headers().get(header::CONTENT_TYPE).unwrap(),
            media_type::REPOSITORY_CONTENT_REQUEST
        );
        assert!(body.contains("xmlns:vfs=\"http://www.sap.com/adt/ris/virtualFolders\""));
        assert!(body.contains("objectSearchPattern=\"Z*\""));
        assert!(body.contains("<vfs:preselection facet=\"PACKAGE\">"));
        assert!(body.contains("<vfs:value>-UI5/STRU</vfs:value>"));
        assert!(body.contains("<vfs:facet>GROUP</vfs:facet>"));
    }

    #[test]
    fn repository_content_response_decodes_one_layer() {
        let query = RepositoryContentQuery::new();
        let response = AdtResponse::new(StatusCode::OK, HeaderMap::new(), CONTENT_XML.to_vec());
        let request_target = AdtUri::parse("/sap/bc/adt/advertised/repository/contents").unwrap();

        let content =
            <RepositoryContentQuery as Operation<Ready>>::decode(&query, response, &request_target)
                .unwrap();

        assert_eq!(content.object_count, 3);
        assert_eq!(content.folders.len(), 1);
        assert_eq!(content.objects.len(), 1);
        assert_eq!(content.objects[0].object_type.to_string(), "CLAS/OC");
        assert_eq!(content.objects[0].relations().len(), 1);
    }

    #[test]
    fn object_properties_request_repeats_included_facets() {
        let client = ready_client(REPOSITORY_DISCOVERY_XML);
        let object = ObjectRef::<Program>::for_test(
            "Z_TEST",
            AdtUri::parse("/sap/bc/adt/programs/programs/z_test").unwrap(),
        );
        let query = RepositoryObjectPropertiesQuery::builder(&object)
            .include_facet(RepositoryFacet::PACKAGE)
            .include_facet(RepositoryFacet::GROUP)
            .build()
            .unwrap();

        let request = query.request(&client).unwrap();

        assert_eq!(request.method(), Method::GET);
        assert_eq!(
            request.target().as_str(),
            "/sap/bc/adt/advertised/repository/object-properties"
        );
        assert_eq!(
            request.query(),
            [
                (
                    "uri".to_owned(),
                    "/sap/bc/adt/programs/programs/z_test".to_owned(),
                ),
                ("facet".to_owned(), "PACKAGE".to_owned()),
                ("facet".to_owned(), "GROUP".to_owned()),
            ]
        );
        assert_eq!(
            request.headers().get(header::ACCEPT).unwrap(),
            media_type::REPOSITORY_OBJECT_PROPERTIES
        );

        let response = AdtResponse::new(
            StatusCode::OK,
            HeaderMap::new(),
            OBJECT_PROPERTIES_XML.to_vec(),
        );
        let properties = <RepositoryObjectPropertiesQuery as Operation<Ready>>::decode(
            &query,
            response,
            request.target(),
        )
        .unwrap();
        assert_eq!(properties.object.reference.uri(), object.uri());
        assert_eq!(properties.properties.len(), 3);
    }

    #[test]
    fn facets_request_uses_the_advertised_collection_target() {
        let client = ready_client(REPOSITORY_DISCOVERY_XML);

        let request = RepositoryFacetsQuery.request(&client).unwrap();

        assert_eq!(
            request.target().as_str(),
            "/sap/bc/adt/advertised/repository/facets"
        );
    }

    #[test]
    fn repository_request_requires_its_discovery_collection() {
        let client = ready_client(
            br#"<app:service xmlns:app="http://www.w3.org/2007/app"
                    xmlns:atom="http://www.w3.org/2005/Atom">
                    <app:workspace><atom:title>Repository</atom:title></app:workspace>
                </app:service>"#,
        );

        let error = RepositoryContentQuery::new().request(&client).unwrap_err();

        assert!(matches!(
            error,
            OperationError::Compatibility(CompatibilityError::MissingCollection(category))
                if category.scheme
                    == "http://www.sap.com/adt/categories/repository/virtualfolders"
                    && category.term == "contents"
        ));
    }
}
