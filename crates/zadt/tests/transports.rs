#![cfg(feature = "reqwest")]

use httpmock::Mock;
use httpmock::prelude::*;
use zadt::{
    AdtUri, Client, Operation, OperationError, QueryTransportKind, Ready, ReqwestTransport,
    ResponseError, TransportCreate, TransportKind, TransportPropertiesQuery, TransportsQuery,
};

const DISCOVERY_XML: &str = include_str!("fixtures/discovery.xml");
const CORE_DISCOVERY_XML: &str = include_str!("fixtures/core-discovery.xml");
const TRANSPORTS_XML: &str = include_str!("fixtures/transport-requests.xml");
const TRANSPORT_XML: &str = include_str!("fixtures/transport-request.xml");
const TRANSPORTS_MEDIA_TYPE: &str =
    "application/vnd.sap.as+xml; charset=UTF-8; dataname=com.sap.adt.CorrectionRequests";
const TRANSPORT_MEDIA_TYPE: &str =
    "application/vnd.sap.as+xml; charset=UTF-8; dataname=com.sap.adt.CorrectionRequest";
const TRANSPORT_CREATE_LEGACY_MEDIA_TYPE: &str =
    "application/vnd.sap.as+xml; charset=UTF-8; dataname=com.sap.adt.CreateCorrectionRequest";
const TRANSPORT_CREATE_V1_MEDIA_TYPE: &str =
    "application/vnd.sap.as+xml; charset=UTF-8; dataname=com.sap.adt.CreateCorrectionRequest.v1";
const TRANSPORT_CREATE_RESULT_MEDIA_TYPE: &str =
    "application/vnd.sap.as+xml; charset=UTF-8; dataname=com.sap.adt.CorrectionRequestResult";
const LEGACY_DISCOVERY_XML: &str = r#"
    <app:service xmlns:app="http://www.w3.org/2007/app"
        xmlns:atom="http://www.w3.org/2005/Atom">
        <app:workspace>
            <atom:title>Change and Transport System</atom:title>
            <app:collection href="/sap/bc/adt/cts/transports">
                <app:accept>application/vnd.sap.as+xml; charset=UTF-8; dataname=com.sap.adt.CreateCorrectionRequest</app:accept>
                <atom:category term="transports" scheme="http://www.sap.com/adt/categories/cts" />
            </app:collection>
        </app:workspace>
    </app:service>
"#;
const TRANSPORT_CREATION_XML: &str = r#"
    <asx:abap version="1.0" xmlns:asx="http://www.sap.com/abapxml">
        <asx:values>
            <DATA>
                <TRKORR>DEVK900003</TRKORR>
                <MESSAGE>
                    <SEVERITY/>
                    <SHORT_TEXT/>
                    <LONG_TEXT/>
                </MESSAGE>
            </DATA>
        </asx:values>
    </asx:abap>
"#;

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
            when.method(GET)
                .path("/sap/bc/adt/core/discovery")
                .header("accept", "application/atomsvc+xml");
            then.status(200).body(CORE_DISCOVERY_XML);
        })
        .await
}

async fn mock_csrf(server: &MockServer) -> Mock<'_> {
    server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/core/discovery")
                .header("x-csrf-token", "Fetch");
            then.status(200).header("x-csrf-token", "CSRF-CTS");
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

#[tokio::test]
async fn transport_creation_prefers_the_v1_asx_contract() {
    let server = MockServer::start_async().await;
    let _discovery = mock_discovery(&server).await;
    let _core_discovery = mock_core_discovery(&server).await;
    let csrf = mock_csrf(&server).await;
    let create = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/sap/bc/adt/cts/transports")
                .query_param("transportLayer", "ZDEV")
                .header("accept", TRANSPORT_CREATE_RESULT_MEDIA_TYPE)
                .header("content-type", TRANSPORT_CREATE_V1_MEDIA_TYPE)
                .header("x-csrf-token", "CSRF-CTS")
                .body_contains("<OPERATION>I</OPERATION>")
                .body_contains("<DEVCLASS>ZPACKAGE</DEVCLASS>")
                .body_contains("<REQUEST_TEXT>Create &amp; test</REQUEST_TEXT>")
                .body_contains("<REF>/sap/bc/adt/packages/zpackage</REF>");
            then.status(201)
                .header("content-type", TRANSPORT_CREATE_RESULT_MEDIA_TYPE)
                .body(TRANSPORT_CREATION_XML);
        })
        .await;

    let client = ready_client(&server).await;
    let creation = TransportCreate::builder()
        .description("Create & test")
        .package("ZPACKAGE")
        .reference(AdtUri::parse("/sap/bc/adt/packages/zpackage").unwrap())
        .transport_layer("ZDEV")
        .build()
        .unwrap()
        .execute(&client)
        .await
        .unwrap();

    assert_eq!(creation.transport_number, "DEVK900003");
    assert_eq!(creation.message, None);
    csrf.assert_async().await;
    create.assert_async().await;
}

#[tokio::test]
async fn transport_creation_falls_back_to_the_legacy_contract() {
    let server = MockServer::start_async().await;
    let _discovery = server
        .mock_async(|when, then| {
            when.method(GET).path("/sap/bc/adt/discovery");
            then.status(200).body(LEGACY_DISCOVERY_XML);
        })
        .await;
    let _core_discovery = mock_core_discovery(&server).await;
    let csrf = mock_csrf(&server).await;
    let create = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/sap/bc/adt/cts/transports")
                .header("accept", "text/plain")
                .header("content-type", TRANSPORT_CREATE_LEGACY_MEDIA_TYPE)
                .header("x-csrf-token", "CSRF-CTS")
                .body_contains("<OPERATION>I</OPERATION>")
                .body_contains("<DEVCLASS>ZPACKAGE</DEVCLASS>")
                .body_contains("<REQUEST_TEXT>Legacy request</REQUEST_TEXT>");
            then.status(200)
                .header("content-type", "text/plain; charset=utf-8")
                .body("/com.sap.cts/object_record/DEVK900004");
        })
        .await;

    let client = ready_client(&server).await;
    let creation = TransportCreate::builder()
        .description("Legacy request")
        .package("ZPACKAGE")
        .build()
        .unwrap()
        .execute(&client)
        .await
        .unwrap();

    assert_eq!(creation.transport_number, "DEVK900004");
    assert_eq!(creation.message, None);
    csrf.assert_async().await;
    create.assert_async().await;
}

#[test]
fn transport_creation_allows_backend_specific_context() {
    TransportCreate::builder()
        .description("Description only")
        .build()
        .unwrap();
    TransportCreate::builder()
        .description("Transport layer only")
        .transport_layer("ZDEV")
        .build()
        .unwrap();
}
