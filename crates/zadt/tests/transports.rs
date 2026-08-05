#![cfg(feature = "reqwest")]

use httpmock::Mock;
use httpmock::prelude::*;
use zadt::{
    Client, Operation, OperationError, QueryTransportKind, Ready, ReqwestTransport, ResponseError,
    TransportKind, TransportPropertiesQuery, TransportsQuery,
};

const DISCOVERY_XML: &str = include_str!("fixtures/discovery.xml");
const CORE_DISCOVERY_XML: &str = include_str!("fixtures/core-discovery.xml");
const TRANSPORTS_XML: &str = include_str!("fixtures/transport-requests.xml");
const TRANSPORT_XML: &str = include_str!("fixtures/transport-request.xml");
const TRANSPORTS_MEDIA_TYPE: &str =
    "application/vnd.sap.as+xml; charset=utf-8; dataname=com.sap.adt.CorrectionRequests";
const TRANSPORT_MEDIA_TYPE: &str =
    "application/vnd.sap.as+xml; charset=utf-8; dataname=com.sap.adt.CorrectionRequest";

async fn mock_discovery(server: &MockServer) -> Mock<'_> {
    server
        .mock_async(|when, then| {
            when.method(GET).path("/sap/bc/adt/discovery");
            then.status(200).body(DISCOVERY_XML);
        })
        .await
}

async fn mock_core_discovery(server: &MockServer) -> Mock<'_> {
    server
        .mock_async(|when, then| {
            when.method(GET).path("/sap/bc/adt/core/discovery");
            then.status(200).body(CORE_DISCOVERY_XML);
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
    Client::new(transport).discover().await.unwrap()
}

#[tokio::test]
async fn wildcard_transport_query_uses_the_discovered_cts_collection() {
    let server = MockServer::start_async().await;
    let discovery = mock_discovery(&server).await;
    let core_discovery = mock_core_discovery(&server).await;
    let transports = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/cts/transports")
                .query_param("_action", "FIND")
                .query_param("trfunction", "*")
                .header("accept", TRANSPORTS_MEDIA_TYPE);
            then.status(200)
                .header("content-type", TRANSPORTS_MEDIA_TYPE)
                .body(TRANSPORTS_XML);
        })
        .await;

    let client = ready_client(&server).await;
    let response = TransportsQuery::builder()
        .kind(QueryTransportKind::All)
        .build()
        .unwrap()
        .execute(&client)
        .await
        .unwrap();

    assert_eq!(response.len(), 2);
    assert_eq!(response.requests[0].number, "DEVK900001");
    assert_eq!(response.requests[0].kind, TransportKind::Workbench);
    discovery.assert_async().await;
    core_discovery.assert_async().await;
    transports.assert_async().await;
}

#[tokio::test]
async fn explicit_user_query_accepts_the_backends_empty_response() {
    let server = MockServer::start_async().await;
    let _discovery = mock_discovery(&server).await;
    let _core_discovery = mock_core_discovery(&server).await;
    let transports = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/cts/transports")
                .query_param("_action", "FIND")
                .query_param("user", "OTHER_USER")
                .query_param("trfunction", "K")
                .header("accept", TRANSPORTS_MEDIA_TYPE);
            then.status(200);
        })
        .await;

    let client = ready_client(&server).await;
    let response = TransportsQuery::builder()
        .user("OTHER_USER")
        .build()
        .unwrap()
        .execute(&client)
        .await
        .unwrap();

    assert!(response.is_empty());
    transports.assert_async().await;
}

#[tokio::test]
async fn transport_properties_use_the_singular_asx_contract() {
    let server = MockServer::start_async().await;
    let _discovery = mock_discovery(&server).await;
    let _core_discovery = mock_core_discovery(&server).await;
    let properties = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/cts/transports/DEVK900001")
                .header("accept", TRANSPORT_MEDIA_TYPE);
            then.status(200)
                .header("content-type", TRANSPORT_MEDIA_TYPE)
                .body(TRANSPORT_XML);
        })
        .await;

    let client = ready_client(&server).await;
    let response = TransportPropertiesQuery::new("DEVK900001")
        .execute(&client)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(response.number, "DEVK900001");
    assert_eq!(response.kind, TransportKind::Workbench);
    assert_eq!(response.client, None);
    assert_eq!(response.properties_query().transport_number(), "DEVK900001");
    properties.assert_async().await;
}

#[tokio::test]
async fn missing_transport_properties_return_none() {
    let server = MockServer::start_async().await;
    let _discovery = mock_discovery(&server).await;
    let _core_discovery = mock_core_discovery(&server).await;
    let properties = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/cts/transports/UNKNOWN")
                .header("accept", TRANSPORT_MEDIA_TYPE);
            then.status(200);
        })
        .await;

    let client = ready_client(&server).await;
    let response = TransportPropertiesQuery::new("UNKNOWN")
        .execute(&client)
        .await
        .unwrap();

    assert_eq!(response, None);
    properties.assert_async().await;
}

#[tokio::test]
async fn transport_properties_reject_the_list_media_type() {
    let server = MockServer::start_async().await;
    let _discovery = mock_discovery(&server).await;
    let _core_discovery = mock_core_discovery(&server).await;
    let _properties = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/cts/transports/DEVK900001");
            then.status(200)
                .header("content-type", TRANSPORTS_MEDIA_TYPE)
                .body(TRANSPORT_XML);
        })
        .await;

    let client = ready_client(&server).await;
    let error = TransportPropertiesQuery::new("DEVK900001")
        .execute(&client)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        OperationError::Response(ResponseError::UnsupportedContentType { content_type, .. })
            if content_type == TRANSPORTS_MEDIA_TYPE
    ));
}

#[tokio::test]
async fn transport_properties_reject_non_success_statuses() {
    let server = MockServer::start_async().await;
    let _discovery = mock_discovery(&server).await;
    let _core_discovery = mock_core_discovery(&server).await;
    let _properties = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/cts/transports/DEVK900001");
            then.status(500).body("CTS failure");
        })
        .await;

    let client = ready_client(&server).await;
    let error = TransportPropertiesQuery::new("DEVK900001")
        .execute(&client)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        OperationError::Response(ResponseError::UnexpectedStatus { status, body })
            if status == 500 && body == "CTS failure"
    ));
}
