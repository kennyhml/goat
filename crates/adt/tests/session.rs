#![cfg(feature = "reqwest")]

use std::time::Duration;

use goat_adt::{Client, LogonError, OperationError, ReqwestTransport, ResponseError};
use httpmock::prelude::*;

const SESSION_XML: &str = include_str!("fixtures/http-session-v3.xml");
const SESSION_MEDIA_TYPE: &str = "application/vnd.sap.adt.core.http.session.v3+xml";

#[tokio::test]
async fn logon_sends_the_v3_contract_and_retains_session_information() {
    let server = MockServer::start_async().await;
    let logon = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/core/http/sessions")
                .query_param_exists("_")
                .header("accept", SESSION_MEDIA_TYPE)
                .header("x-sap-security-session", "create")
                .header("sap-adt-purpose", "logon")
                .header("sap-adt-saplb", "fetch")
                .header("sap-cancel-on-close", "true")
                .header("cookie", "sap-usercontext=sap-client=001&sap-language=EN")
                .header("authorization", "Basic VVNFUjpQQVNTV09SRA==");
            then.status(200)
                .header("content-type", SESSION_MEDIA_TYPE)
                .body(SESSION_XML);
        })
        .await;
    let transport = ReqwestTransport::builder()
        .destination(server.base_url())
        .sap_client("001")
        .language("EN")
        .basic_auth("USER", "PASSWORD")
        .build()
        .unwrap();

    let client = Client::new(transport).logon().await.unwrap();
    let session = client.session_information();

    assert_eq!(session.logoff_uri.as_str(), "/sap/public/bc/icf/logoff");
    assert_eq!(
        session.cleanup_uri.as_str(),
        "/sap/bc/adt/core/http/sessions/security-context"
    );
    assert_eq!(session.inactivity_timeout, Some(Duration::from_secs(3600)));
    assert_eq!(
        session.system_information.as_ref().unwrap().target.as_str(),
        "/sap/bc/adt/core/http/systeminformation"
    );
    logon.assert_async().await;
}

#[tokio::test]
async fn logon_rejects_legacy_session_representations() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/sap/bc/adt/core/http/sessions");
            then.status(200)
                .header(
                    "content-type",
                    "application/vnd.sap.adt.core.http.session.v2+xml",
                )
                .body(SESSION_XML);
        })
        .await;
    let transport = ReqwestTransport::builder()
        .destination(server.base_url())
        .sap_client("001")
        .language("EN")
        .basic_auth("USER", "PASSWORD")
        .build()
        .unwrap();

    let error = match Client::new(transport).logon().await {
        Ok(_) => panic!("legacy session representation was accepted"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        OperationError::Response(ResponseError::Logon(
            LogonError::UnsupportedContentType { .. }
        ))
    ));
}
