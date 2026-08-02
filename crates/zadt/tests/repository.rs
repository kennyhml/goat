#![cfg(feature = "reqwest")]

use httpmock::prelude::*;
use zadt::{
    Client, Operation, RepositoryContentOperation, RepositoryContentQuery, RepositoryFacet,
    RepositoryFacetsQuery, RepositoryPreselection, ReqwestTransport,
};

const CONTENT_XML: &str = include_str!("fixtures/repository-content.xml");
const FACETS_XML: &str = include_str!("fixtures/repository-facets.xml");
const OBJECT_PROPERTIES_XML: &str = include_str!("fixtures/repository-object-properties.xml");
const DISCOVERY_XML: &str = include_str!("fixtures/discovery.xml");
const CORE_DISCOVERY_XML: &str = include_str!("fixtures/core-discovery.xml");

#[tokio::test]
async fn repository_queries_use_discovered_collections() {
    let server = MockServer::start_async().await;
    let discovery = server
        .mock_async(|when, then| {
            when.method(GET).path("/sap/bc/adt/discovery");
            then.status(200).body(DISCOVERY_XML);
        })
        .await;
    let _core_discovery = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/core/discovery")
                .header("accept", "application/atomsvc+xml");
            then.status(200).body(CORE_DISCOVERY_XML);
        })
        .await;
    let csrf = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/core/discovery")
                .header("x-csrf-token", "Fetch");
            then.status(200).header("x-csrf-token", "CSRF-TOKEN-RIS");
        })
        .await;
    let contents = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/sap/bc/adt/repository/informationsystem/virtualfolders/contents")
                .query_param("operation", "expand")
                .query_param("ignoreShortDescriptions", "false")
                .header(
                    "content-type",
                    "application/vnd.sap.adt.repository.virtualfolders.request.v1+xml",
                )
                .header("x-csrf-token", "CSRF-TOKEN-RIS")
                .body_contains("objectSearchPattern=\"Z*\"")
                .body_contains("<vfs:preselection facet=\"PACKAGE\">")
                .body_contains("<vfs:value>$TMP</vfs:value>")
                .body_contains("<vfs:facet>GROUP</vfs:facet>");
            then.status(200).body(CONTENT_XML);
        })
        .await;
    let facets = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/repository/informationsystem/virtualfolders/facets");
            then.status(200).body(FACETS_XML);
        })
        .await;
    let properties = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/repository/informationsystem/objectproperties/values")
                .query_param("uri", "/sap/bc/adt/oo/classes/zcl_demo")
                .query_param("facet", "PACKAGE")
                .header(
                    "accept",
                    "application/vnd.sap.adt.repository.objproperties.result.v1+xml",
                );
            then.status(200).body(OBJECT_PROPERTIES_XML);
        })
        .await;
    let transport = ReqwestTransport::builder()
        .destination(server.base_url())
        .sap_client("001")
        .language("EN")
        .basic_auth("USER", "PASSWORD")
        .build()
        .unwrap();
    let client = Client::new(transport).discover().await.unwrap();

    let content = RepositoryContentQuery::builder()
        .search_pattern("Z*")
        .preselection(RepositoryPreselection::new(
            RepositoryFacet::PACKAGE,
            "$TMP",
        ))
        .facet(RepositoryFacet::GROUP)
        .operation(RepositoryContentOperation::Expand)
        .ignore_short_descriptions(false)
        .build()
        .unwrap()
        .execute(&client)
        .await
        .unwrap();
    let available_facets = RepositoryFacetsQuery.execute(&client).await.unwrap();
    let object_properties = content.objects[0]
        .properties_builder()
        .include_facet(RepositoryFacet::PACKAGE)
        .build()
        .unwrap()
        .execute(&client)
        .await
        .unwrap();

    assert_eq!(content.folders[0].display_name, "Source Code Library");
    assert_eq!(content.objects[0].name, "ZCL_DEMO");
    assert_eq!(available_facets.facets[0].key, "appl");
    assert_eq!(object_properties.object.name, "CL_ADT_URI_MAPPER");
    discovery.assert_async().await;
    csrf.assert_async().await;
    contents.assert_async().await;
    facets.assert_async().await;
    properties.assert_async().await;
}
