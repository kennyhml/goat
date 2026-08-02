#![cfg(feature = "reqwest")]

use async_trait::async_trait;
use http::{HeaderMap, HeaderValue, StatusCode, header};
use httpmock::Mock;
use httpmock::prelude::*;
use std::sync::{Arc, Mutex};
use zadt::{
    AdtRequest, AdtResponse, CategoryId, Class, Client, CoreDiscoveryQuery, DiscoveryError,
    DiscoveryQuery, Logon, Operation, OperationError, ReqwestTransport, ResponseError, Transport,
    TransportError,
};

const DISCOVERY_XML: &str = include_str!("fixtures/discovery.xml");
const CORE_DISCOVERY_XML: &str = include_str!("fixtures/core-discovery.xml");
const INVALID_DISCOVERY_XML: &str = include_str!("fixtures/invalid-discovery.xml");
const SESSION_XML: &str = include_str!("fixtures/http-session-v3.xml");
const SESSION_MEDIA_TYPE: &str = "application/vnd.sap.adt.core.http.session.v3+xml";
const PROGRAMS_SCHEME: &str = "http://www.sap.com/adt/categories/programs";
const PROGRAMS_CATEGORY: CategoryId = CategoryId {
    scheme: PROGRAMS_SCHEME,
    term: "programs",
};
const COMPATIBILITY_SCHEME: &str = "http://www.sap.com/adt/categories/compatibility";

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

#[tokio::test]
async fn core_discovery_is_available_before_central_discovery() {
    let transport = FixtureTransport::new(CORE_DISCOVERY_XML);
    let requests = Arc::clone(&transport.requests);
    let client = Client::new(transport);
    Logon.execute(&client).await.unwrap();

    let capabilities = CoreDiscoveryQuery.execute(&client).await.unwrap();
    let collection = capabilities
        .collection(COMPATIBILITY_SCHEME, "graph")
        .unwrap();

    assert_eq!(
        collection.target().as_str(),
        "/sap/bc/adt/compatibility/graph"
    );
    assert!(
        capabilities
            .collection(PROGRAMS_SCHEME, "programs")
            .is_none()
    );
    assert_eq!(
        requests.lock().unwrap().as_slice(),
        [
            "/sap/bc/adt/core/http/sessions",
            "/sap/bc/adt/core/discovery"
        ]
    );
}

#[tokio::test]
async fn client_discovery_transitions_and_retains_capabilities() {
    let transport = FixtureTransport::new(DISCOVERY_XML);
    let requests = Arc::clone(&transport.requests);
    let client = Client::new(transport);
    Logon.execute(&client).await.unwrap();
    let client = client.discover().await.unwrap();
    let cloned_client = client.clone();

    let collection = client.collection(PROGRAMS_CATEGORY).unwrap();

    assert_eq!(collection.title(), Some("Programs"));
    assert_eq!(
        collection.accepted_media_types(),
        [
            "application/vnd.sap.adt.programs.programs.v2+xml",
            "application/vnd.sap.adt.programs.programs.v3+xml",
        ]
    );
    assert_eq!(collection.template_links().len(), 1);
    assert!(std::ptr::eq(
        client.capabilities(),
        cloned_client.capabilities()
    ));
    assert!(std::ptr::eq(
        client.core_capabilities(),
        cloned_client.core_capabilities()
    ));
    assert!(
        client
            .core_capabilities()
            .collection(
                "http://www.sap.com/adt/categories/system/communication/services",
                "batch"
            )
            .is_some()
    );
    assert_eq!(
        requests.lock().unwrap().as_slice(),
        [
            "/sap/bc/adt/core/http/sessions",
            "/sap/bc/adt/discovery",
            "/sap/bc/adt/core/discovery",
        ]
    );
}

#[tokio::test]
async fn class_references_use_the_discovered_oo_collection() {
    let client = Client::new(FixtureTransport::new(DISCOVERY_XML));
    Logon.execute(&client).await.unwrap();
    let client = client.discover().await.unwrap();

    let class = client.object::<Class>("ZCL_EXAMPLE").unwrap();

    assert_eq!(class.name(), "ZCL_EXAMPLE");
    assert_eq!(class.uri().as_str(), "/sap/bc/adt/oo/classes/zcl_example");
}

#[tokio::test]
async fn reqwest_transport_sends_the_discovery_contract() {
    let server = MockServer::start_async().await;
    let logon = mock_logon(&server).await;
    let discovery = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/discovery")
                .header("accept", "application/atomsvc+xml")
                .header("cookie", "sap-usercontext=sap-client=001&sap-language=EN")
                .header("authorization", "Basic VVNFUjpQQVNTV09SRA==");
            then.status(200)
                .header("content-type", "application/atomsvc+xml")
                .body(DISCOVERY_XML);
        })
        .await;
    let core_discovery = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/core/discovery")
                .header("accept", "application/atomsvc+xml")
                .header("cookie", "sap-usercontext=sap-client=001&sap-language=EN")
                .header("authorization", "Basic VVNFUjpQQVNTV09SRA==");
            then.status(200)
                .header("content-type", "application/atomsvc+xml")
                .body(CORE_DISCOVERY_XML);
        })
        .await;

    let transport = ReqwestTransport::builder()
        .destination(server.base_url())
        .sap_client("001")
        .language("EN")
        .basic_auth("USER", "PASSWORD")
        .build()
        .unwrap();

    let client = Client::new(transport);
    Logon.execute(&client).await.unwrap();
    let client = client.discover().await.unwrap();

    logon.assert_async().await;
    discovery.assert_async().await;
    core_discovery.assert_async().await;
    assert!(
        client
            .capabilities()
            .collection(PROGRAMS_SCHEME, "programs")
            .is_some()
    );
}

#[tokio::test]
async fn reqwest_transport_reuses_security_session_cookies() {
    let server = MockServer::start_async().await;
    let logon = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/core/http/sessions")
                .header("cookie", "sap-usercontext=sap-client=001&sap-language=EN");
            then.status(200)
                .header("set-cookie", "SAP_SESSIONID_A4H_001=session; Path=/")
                .header("content-type", SESSION_MEDIA_TYPE)
                .body(SESSION_XML);
        })
        .await;
    let central_discovery_user_context_first = server
        .mock_async(|when, then| {
            when.method(GET).path("/sap/bc/adt/discovery").header(
                "cookie",
                "sap-usercontext=sap-client=001&sap-language=EN; SAP_SESSIONID_A4H_001=session",
            );
            then.status(200).body(DISCOVERY_XML);
        })
        .await;
    let central_discovery_session_first = server
        .mock_async(|when, then| {
            when.method(GET).path("/sap/bc/adt/discovery").header(
                "cookie",
                "SAP_SESSIONID_A4H_001=session; sap-usercontext=sap-client=001&sap-language=EN",
            );
            then.status(200).body(DISCOVERY_XML);
        })
        .await;
    let _core_discovery = server
        .mock_async(|when, then| {
            when.method(GET).path("/sap/bc/adt/core/discovery");
            then.status(200).body(CORE_DISCOVERY_XML);
        })
        .await;
    let transport = ReqwestTransport::builder()
        .destination(server.base_url())
        .sap_client("001")
        .language("EN")
        .basic_auth("USER", "PASSWORD")
        .build()
        .unwrap();
    let client = Client::new(transport);
    Logon.execute(&client).await.unwrap();

    client.discover().await.unwrap();

    logon.assert_async().await;
    assert_eq!(
        central_discovery_user_context_first.hits_async().await
            + central_discovery_session_first.hits_async().await,
        1
    );
}

#[tokio::test]
async fn unexpected_status_is_an_operation_response_error() {
    let server = MockServer::start_async().await;
    let logon = mock_logon(&server).await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/sap/bc/adt/discovery");
            then.status(401).body("authentication required");
        })
        .await;

    let transport = ReqwestTransport::builder()
        .destination(server.base_url())
        .sap_client("001")
        .language("EN")
        .basic_auth("USER", "WRONG")
        .build()
        .unwrap();

    let client = Client::new(transport);
    Logon.execute(&client).await.unwrap();
    let error = match client.discover().await {
        Ok(_) => panic!("discovery unexpectedly succeeded"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        OperationError::Response(ResponseError::UnexpectedStatus {
            status: StatusCode::UNAUTHORIZED,
            ..
        })
    ));
    logon.assert_async().await;
}

#[tokio::test]
async fn discovery_rejects_collection_urls_outside_the_sap_resource_root() {
    let client = Client::new(FixtureTransport::new(INVALID_DISCOVERY_XML));
    Logon.execute(&client).await.unwrap();
    let error = DiscoveryQuery.execute(&client).await.unwrap_err();

    assert!(matches!(
        error,
        OperationError::Response(ResponseError::Discovery(
            DiscoveryError::InvalidCollectionHref { .. }
        ))
    ));
}

struct FixtureTransport {
    response: &'static str,
    requests: Arc<Mutex<Vec<String>>>,
}

impl FixtureTransport {
    fn new(response: &'static str) -> Self {
        Self {
            response,
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl Transport for FixtureTransport {
    async fn send(&self, request: AdtRequest) -> Result<AdtResponse, TransportError> {
        self.requests
            .lock()
            .unwrap()
            .push(request.target().as_str().to_owned());
        if request.target().as_str() == "/sap/bc/adt/core/http/sessions" {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(SESSION_MEDIA_TYPE),
            );
            return Ok(AdtResponse::new(
                StatusCode::OK,
                headers,
                SESSION_XML.as_bytes().to_vec(),
            ));
        }
        let response = if request.target().as_str() == "/sap/bc/adt/core/discovery" {
            CORE_DISCOVERY_XML
        } else {
            self.response
        };
        Ok(AdtResponse::new(
            StatusCode::OK,
            HeaderMap::new(),
            response.as_bytes().to_vec(),
        ))
    }
}
