use async_trait::async_trait;
use goat_adt::{
    AdtRequest, AdtResponse, Client, CoreDiscoveryQuery, DiscoveryError, DiscoveryQuery, Operation,
    OperationError, ReqwestTransport, ResponseError, Transport, TransportError,
};
use http::{HeaderMap, StatusCode};
use httpmock::prelude::*;
use std::sync::{Arc, Mutex};

const DISCOVERY_XML: &str = include_str!("fixtures/discovery.xml");
const CORE_DISCOVERY_XML: &str = include_str!("fixtures/core-discovery.xml");
const INVALID_DISCOVERY_XML: &str = include_str!("fixtures/invalid-discovery.xml");
const PROGRAMS_SCHEME: &str = "http://www.sap.com/adt/categories/programs";
const COMPATIBILITY_SCHEME: &str = "http://www.sap.com/adt/categories/compatibility";

#[tokio::test]
async fn discovery_is_an_operation_for_an_undiscovered_client() {
    let transport = FixtureTransport::new(CORE_DISCOVERY_XML);
    let requests = Arc::clone(&transport.requests);
    let client = Client::new(transport);

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
        ["/sap/bc/adt/core/discovery"]
    );
}

#[tokio::test]
async fn client_discovery_transitions_and_retains_capabilities() {
    let client = Client::new(FixtureTransport::new(DISCOVERY_XML))
        .discover()
        .await
        .unwrap();

    let collection = client
        .capabilities()
        .collection(PROGRAMS_SCHEME, "programs")
        .unwrap();

    assert_eq!(collection.title(), Some("Programs"));
    assert_eq!(
        collection.accepted_media_types(),
        [
            "application/vnd.sap.adt.programs.programs.v2+xml",
            "application/vnd.sap.adt.programs.programs.v3+xml",
        ]
    );
    assert_eq!(collection.template_links().len(), 1);
}

#[tokio::test]
async fn reqwest_transport_sends_the_discovery_contract() {
    let server = MockServer::start_async().await;
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

    let transport = ReqwestTransport::builder()
        .destination(server.base_url())
        .sap_client("001")
        .language("EN")
        .basic_auth("USER", "PASSWORD")
        .build()
        .unwrap();

    let client = Client::new(transport).discover().await.unwrap();

    discovery.assert_async().await;
    assert!(
        client
            .capabilities()
            .collection(PROGRAMS_SCHEME, "programs")
            .is_some()
    );
}

#[tokio::test]
async fn unexpected_status_is_an_operation_response_error() {
    let server = MockServer::start_async().await;
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

    let error = match Client::new(transport).discover().await {
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
}

#[tokio::test]
async fn discovery_rejects_collection_urls_outside_the_sap_resource_root() {
    let error = DiscoveryQuery
        .execute(&Client::new(FixtureTransport::new(INVALID_DISCOVERY_XML)))
        .await
        .unwrap_err();

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
        Ok(AdtResponse::new(
            StatusCode::OK,
            HeaderMap::new(),
            self.response.as_bytes().to_vec(),
        ))
    }
}
