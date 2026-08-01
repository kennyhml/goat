#![cfg(feature = "reqwest")]

use httpmock::Mock;
use httpmock::prelude::*;
use zadt::{
    Client, Logon, Operation, Package, PackageProperties, PackagePropertiesVersion,
    PackageSettingsQuery, Ready, ReqwestTransport,
};

const DISCOVERY_XML: &str = include_str!("fixtures/discovery.xml");
const PACKAGE_XML: &str = include_str!("fixtures/package-sadt-tools-core.xml");
const SUPER_TREE_XML: &str = include_str!("fixtures/package-tree-super.xml");
const SUB_TREE_XML: &str = include_str!("fixtures/package-tree-sub.xml");
const SETTINGS_XML: &str = include_str!("fixtures/package-settings.xml");
const SESSION_XML: &str = include_str!("fixtures/http-session-v3.xml");
const SESSION_MEDIA_TYPE: &str = "application/vnd.sap.adt.core.http.session.v3+xml";

async fn mock_logon(server: &MockServer) -> Mock<'_> {
    server
        .mock_async(|when, then| {
            when.method(GET).path("/sap/bc/adt/core/http/sessions");
            then.status(200)
                .header("content-type", SESSION_MEDIA_TYPE)
                .body(SESSION_XML);
        })
        .await
}

async fn mock_discovery(server: &MockServer) -> Mock<'_> {
    server
        .mock_async(|when, then| {
            when.method(GET).path("/sap/bc/adt/discovery");
            then.status(200).body(DISCOVERY_XML);
        })
        .await
}

async fn ready_client(server: &MockServer) -> Client<Ready> {
    let transport = ReqwestTransport::builder()
        .destination(server.base_url())
        .sap_client("001")
        .language("EN")
        .basic_auth("USER", "PASSWORD")
        .build()
        .unwrap();
    let client = Client::new(transport);
    Logon.execute(&client).await.unwrap();
    client.discover().await.unwrap()
}

#[tokio::test]
async fn package_properties_use_the_discovered_v2_contract() {
    let server = MockServer::start_async().await;
    let logon = mock_logon(&server).await;
    let discovery = mock_discovery(&server).await;
    let properties = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/packages/sadt_tools_core")
                .header("accept", "application/vnd.sap.adt.packages.v2+xml")
                .header("cache-control", "no-cache");
            then.status(200)
                .header(
                    "content-type",
                    "application/vnd.sap.adt.packages.v2+xml; charset=utf-8",
                )
                .header("etag", "package-etag")
                .body(PACKAGE_XML);
        })
        .await;

    let client = ready_client(&server).await;
    let reference = client.object::<Package>("sadt_tools_core").unwrap();
    let response = reference.query().execute(&client).await.unwrap();
    assert_eq!(response.media_version(), PackagePropertiesVersion::V2);
    let PackageProperties::V2(package) = response else {
        panic!("unexpected package-properties version");
    };

    assert_eq!(package.reference, reference);
    assert_eq!(package.name, "SADT_TOOLS_CORE");
    assert_eq!(package.etag.as_deref(), Some("package-etag"));
    assert_eq!(package.package_interfaces.len(), 1);
    assert_eq!(package.use_accesses.len(), 1);

    logon.assert_async().await;
    discovery.assert_async().await;
    properties.assert_async().await;
}

#[tokio::test]
async fn package_tree_queries_expand_the_discovered_template() {
    let server = MockServer::start_async().await;
    let logon = mock_logon(&server).await;
    let discovery = mock_discovery(&server).await;
    let super_tree = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/packages/$tree")
                .query_param("packagename", "SADT_TOOLS_CORE")
                .query_param("type", "super")
                .header("accept", "application/vnd.sap.adt.packages.tree.v1+xml");
            then.status(200)
                .header(
                    "content-type",
                    "application/vnd.sap.adt.packages.tree.v1+xml; charset=utf-8",
                )
                .body(SUPER_TREE_XML);
        })
        .await;
    let sub_tree = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/packages/$tree")
                .query_param("packagename", "SADT_TOOLS_CORE")
                .query_param("type", "sub")
                .header("accept", "application/vnd.sap.adt.packages.tree.v1+xml");
            then.status(200)
                .header(
                    "content-type",
                    "application/vnd.sap.adt.packages.tree.v1+xml; charset=utf-8",
                )
                .body(SUB_TREE_XML);
        })
        .await;

    let client = ready_client(&server).await;
    let package = client.object::<Package>("SADT_TOOLS_CORE").unwrap();
    let ancestors = package.super_tree().execute(&client).await.unwrap();
    let children = package.sub_tree().execute(&client).await.unwrap();

    assert!(ancestors.is_super_tree);
    assert_eq!(ancestors.nodes.len(), 2);
    assert!(!children.is_super_tree);
    assert_eq!(children.nodes.len(), 1);

    logon.assert_async().await;
    discovery.assert_async().await;
    super_tree.assert_async().await;
    sub_tree.assert_async().await;
}

#[tokio::test]
async fn package_settings_use_the_discovered_collection() {
    let server = MockServer::start_async().await;
    let logon = mock_logon(&server).await;
    let discovery = mock_discovery(&server).await;
    let settings = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/packages/settings")
                .header("accept", "application/vnd.sap.adt.packages.settings+xml");
            then.status(200)
                .header(
                    "content-type",
                    "application/vnd.sap.adt.packages.settings+xml; charset=utf-8",
                )
                .body(SETTINGS_XML);
        })
        .await;

    let client = ready_client(&server).await;
    let response = PackageSettingsQuery.execute(&client).await.unwrap();

    assert!(!response.show_package_check_errors);
    logon.assert_async().await;
    discovery.assert_async().await;
    settings.assert_async().await;
}
