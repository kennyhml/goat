use goat_adt::{AccessMode, Client, Operation, ReqwestTransport};
use httpmock::prelude::*;

const DISCOVERY_XML: &str = include_str!("fixtures/discovery.xml");
const LOCK_XML: &str = include_str!("fixtures/object-lock.xml");
const SOURCE: &str = "REPORT z_goat_test.\nWRITE / 'updated'.\n";

#[tokio::test]
async fn program_lock_and_update_share_one_user_session() {
    let server = MockServer::start_async().await;
    let discovery = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/discovery")
                .header("cookie", "sap-usercontext=sap-client=001&sap-language=EN");
            then.status(200).body(DISCOVERY_XML);
        })
        .await;
    let csrf = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/core/discovery")
                .header("x-csrf-token", "Fetch")
                .header("cookie", "sap-usercontext=sap-client=001&sap-language=EN");
            then.status(200).header("x-csrf-token", "CSRF-TOKEN-1");
        })
        .await;
    let get_source = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/programs/programs/Z_GOAT_TEST/source/main")
                .header("accept", "text/plain");
            then.status(200)
                .header("etag", "SOURCE-ETAG-1")
                .body(SOURCE);
        })
        .await;
    let lock_program = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/sap/bc/adt/programs/programs/Z_GOAT_TEST")
                .query_param("_action", "LOCK")
                .query_param("accessMode", "MODIFY")
                .header(
                    "accept",
                    "application/vnd.sap.as+xml; charset=utf-8; dataname=com.sap.adt.lock.Result2",
                )
                .header("x-sap-adt-sessiontype", "stateful")
                .header("x-csrf-token", "CSRF-TOKEN-1")
                .header("cookie", "sap-usercontext=sap-client=001&sap-language=EN");
            then.status(200)
                .header(
                    "set-cookie",
                    "sap-contextid=USER-SESSION-1; Path=/sap/bc/adt",
                )
                .body(LOCK_XML);
        })
        .await;
    let update_source = server
        .mock_async(|when, then| {
            when.method(PUT)
                .path("/sap/bc/adt/programs/programs/Z_GOAT_TEST/source/main")
                .query_param("lockHandle", "LOCK-HANDLE-1")
                .header("content-type", "text/plain; charset=utf-8")
                .header("x-sap-adt-sessiontype", "stateful")
                .header("x-csrf-token", "CSRF-TOKEN-1")
                .header(
                    "cookie",
                    "sap-usercontext=sap-client=001&sap-language=EN; sap-contextid=USER-SESSION-1",
                )
                .body(SOURCE);
            then.status(200);
        })
        .await;
    let unlock_program = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/sap/bc/adt/programs/programs/Z_GOAT_TEST")
                .query_param("_action", "UNLOCK")
                .query_param("lockHandle", "LOCK-HANDLE-1")
                .header("x-sap-adt-sessiontype", "stateful")
                .header("x-csrf-token", "CSRF-TOKEN-1")
                .header(
                    "cookie",
                    "sap-usercontext=sap-client=001&sap-language=EN; sap-contextid=USER-SESSION-1",
                );
            then.status(200);
        })
        .await;
    let close_session = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/core/discovery")
                .header("x-sap-adt-sessiontype", "stateless")
                .header(
                    "cookie",
                    "sap-usercontext=sap-client=001&sap-language=EN; sap-contextid=USER-SESSION-1",
                );
            then.status(200);
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
    let program = client.program("Z_GOAT_TEST").unwrap();
    let source = program.source().query().execute(&client).await.unwrap();
    let session = client.create_user_session();

    let lock_handle = program
        .lock(AccessMode::Modify)
        .execute(&session)
        .await
        .unwrap();
    program
        .source()
        .update()
        .lock_handle(lock_handle.clone())
        .content(source.content.as_str())
        .build()
        .unwrap()
        .execute(&session)
        .await
        .unwrap();
    assert_eq!(&lock_handle.object, program.object());
    assert_eq!(lock_handle.handle, "LOCK-HANDLE-1");
    program
        .unlock(lock_handle)
        .unwrap()
        .execute(&session)
        .await
        .unwrap();
    session.close().await.unwrap();

    assert_eq!(source.etag.as_deref(), Some("SOURCE-ETAG-1"));
    discovery.assert_async().await;
    csrf.assert_async().await;
    get_source.assert_async().await;
    lock_program.assert_async().await;
    update_source.assert_async().await;
    unlock_program.assert_async().await;
    close_session.assert_async().await;
}
