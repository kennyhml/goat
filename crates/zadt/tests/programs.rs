#![cfg(feature = "reqwest")]

use httpmock::Mock;
use httpmock::prelude::*;
use zadt::{
    AccessMode, Client, EntityTag, Include, IncludeProperties, IncludePropertyVersion, Logon,
    ObjectType, ObjectVersion, Operation, Package, Program, ProgramProperties,
    ProgramPropertiesVersion, Ready, ReqwestTransport, Revalidation,
};

const DISCOVERY_XML: &str = include_str!("fixtures/discovery.xml");
const CORE_DISCOVERY_XML: &str = include_str!("fixtures/core-discovery.xml");
const LOCK_XML: &str = include_str!("fixtures/object-lock.xml");
// Captured from live A4H. Its V2 and V3 response bodies were byte-identical.
const PROGRAM_XML: &str = include_str!("fixtures/program-z-test.xml");
const INCLUDE_XML: &str = include_str!("fixtures/include-ztest.xml");
const SESSION_XML: &str = include_str!("fixtures/http-session-v3.xml");
const SESSION_MEDIA_TYPE: &str = "application/vnd.sap.adt.core.http.session.v3+xml";
const SOURCE: &str = "REPORT z_ziege_test.\nWRITE / 'updated'.\n";
const RUN_OUTPUT: &str = "Hello from Z_TEST\n";

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

async fn ready_client(transport: ReqwestTransport) -> Client<Ready> {
    let client = Client::new(transport);
    Logon.execute(&client).await.unwrap();
    client.discover().await.unwrap()
}

#[tokio::test]
async fn program_run_uses_the_advertised_profiled_template() {
    let server = MockServer::start_async().await;
    let logon = mock_logon(&server).await;
    let _core_discovery = mock_core_discovery(&server).await;
    let discovery = server
        .mock_async(|when, then| {
            when.method(GET).path("/sap/bc/adt/discovery");
            then.status(200).body(DISCOVERY_XML);
        })
        .await;
    let csrf = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/core/discovery")
                .header("x-csrf-token", "Fetch")
                .header("x-sap-adt-sessiontype", "stateless");
            then.status(200).header("x-csrf-token", "CSRF-TOKEN-RUN");
        })
        .await;
    let run = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/sap/bc/adt/programs/programrun/z_test")
                .query_param("profilerId", "TRACE ID")
                .header("accept", "text/plain")
                .header("x-csrf-token", "CSRF-TOKEN-RUN")
                .body("");
            then.status(200)
                .header("content-type", "text/plain; charset=utf-8")
                .body(RUN_OUTPUT);
        })
        .await;
    let transport = ReqwestTransport::builder()
        .destination(server.base_url())
        .sap_client("001")
        .language("EN")
        .basic_auth("USER", "PASSWORD")
        .build()
        .unwrap();

    let client = ready_client(transport).await;
    let program = client.object::<Program>("z_test").unwrap();
    let output = program
        .run()
        .profiler_id("TRACE ID")
        .build()
        .unwrap()
        .execute(&client)
        .await
        .unwrap();

    assert_eq!(program.name(), "Z_TEST");
    assert_eq!(output.reference, program);
    assert_eq!(output.content, RUN_OUTPUT);
    logon.assert_async().await;
    discovery.assert_async().await;
    csrf.assert_async().await;
    run.assert_async().await;
}

#[tokio::test]
async fn include_properties_query_converts_the_live_ztest_properties() {
    let server = MockServer::start_async().await;
    let logon = mock_logon(&server).await;
    let _core_discovery = mock_core_discovery(&server).await;
    let discovery = server
        .mock_async(|when, then| {
            when.method(GET).path("/sap/bc/adt/discovery");
            then.status(200).body(DISCOVERY_XML);
        })
        .await;
    let metadata = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/programs/includes/ztest")
                .query_param("version", "active")
                .header("accept", "application/vnd.sap.adt.programs.includes.v2+xml")
                .header("cache-control", "no-cache");
            then.status(200)
                .header(
                    "content-type",
                    "application/vnd.sap.adt.programs.includes.v2+xml; charset=utf-8",
                )
                .header("etag", "2026012416174900180")
                .body(INCLUDE_XML);
        })
        .await;
    let get_source = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/programs/includes/ztest/source/main")
                .header("accept", "text/plain");
            then.status(200).body(SOURCE);
        })
        .await;
    let transport = ReqwestTransport::builder()
        .destination(server.base_url())
        .sap_client("001")
        .language("EN")
        .basic_auth("USER", "PASSWORD")
        .build()
        .unwrap();

    let client = ready_client(transport).await;
    let reference = client.object::<Include>("ZTEST").unwrap();
    let response = reference
        .query()
        .priority([IncludePropertyVersion::V2])
        .version(ObjectVersion::Active)
        .execute(&client)
        .await
        .unwrap();
    assert_eq!(response.media_version(), IncludePropertyVersion::V2);
    let include = match response {
        IncludeProperties::V2(include) => *include,
        _ => panic!("unexpected include-properties version"),
    };
    let source = include.source.query().execute(&client).await.unwrap();

    assert_eq!(include.reference, reference);
    assert_eq!(include.name, "ZTEST");
    assert_eq!(include.object_type.to_string(), "PROG/I");
    assert_eq!(include.version, ObjectVersion::Active);
    assert_eq!(include.context_ref_count, 0);
    assert_eq!(include.package.name(), "$TMP");
    assert_eq!(include.relations().len(), 7);
    assert_eq!(include.etag.as_deref(), Some("2026012416174900180"));
    assert_eq!(source.content, SOURCE);

    logon.assert_async().await;
    discovery.assert_async().await;
    metadata.assert_async().await;
    get_source.assert_async().await;
}

#[tokio::test]
async fn program_properties_query_converts_the_live_z_test_v3_properties() {
    let server = MockServer::start_async().await;
    let logon = mock_logon(&server).await;
    let _core_discovery = mock_core_discovery(&server).await;
    let discovery = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/discovery")
                .header("cookie", "sap-usercontext=sap-client=001&sap-language=EN");
            then.status(200).body(DISCOVERY_XML);
        })
        .await;
    let metadata = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/programs/programs/z_test")
                .header("accept", "application/vnd.sap.adt.programs.programs.v3+xml")
                .header("cache-control", "no-cache");
            then.status(200)
                .header(
                    "content-type",
                    "application/vnd.sap.adt.programs.programs.v3+xml; charset=utf-8",
                )
                .header("etag", "202607251959580008")
                .body(PROGRAM_XML);
        })
        .await;
    let get_source = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/programs/programs/z_test/source/main")
                .header("accept", "text/plain");
            then.status(200).body(SOURCE);
        })
        .await;
    let transport = ReqwestTransport::builder()
        .destination(server.base_url())
        .sap_client("001")
        .language("EN")
        .basic_auth("USER", "PASSWORD")
        .build()
        .unwrap();

    let client = ready_client(transport).await;
    let reference = client.object::<Program>("Z_TEST").unwrap();
    let response = reference.query().execute(&client).await.unwrap();
    assert_eq!(response.media_version(), ProgramPropertiesVersion::V3);
    let program = match response {
        ProgramProperties::V3(program) => *program,
        _ => panic!("unexpected program-properties version"),
    };
    let source = program.source.query().execute(&client).await.unwrap();

    assert_eq!(program.reference, reference);
    assert_eq!(program.name, "Z_TEST");
    assert_eq!(program.object_type.to_string(), "PROG/P");
    assert_eq!(program.version, ObjectVersion::Inactive);
    assert_eq!(program.program_type, "executableProgram");
    assert!(program.fix_point_arithmetic);
    assert!(program.unicode_check_active);
    assert_eq!(program.package.name(), "$TMP");
    assert_eq!(Package::WORKBENCH_TYPE.to_string(), "DEVC/K");
    assert_eq!(
        program.package.uri().as_str(),
        "/sap/bc/adt/packages/%24tmp"
    );
    assert_eq!(program.syntax_configuration.language.version, "X");
    assert_eq!(
        program.syntax_configuration.language.description,
        "Standard ABAP"
    );
    assert_eq!(program.relations().len(), 9);
    let links = program
        .relations()
        .iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let text_elements_link = links
        .iter()
        .find(|link| {
            link.relation.as_deref()
                == Some("http://www.sap.com/adt/relations/sources/textelements")
        })
        .unwrap();
    assert_eq!(text_elements_link.title.as_deref(), Some("Text Elements"));
    assert_eq!(
        text_elements_link.target.as_str(),
        "/sap/bc/adt/textelements/programs/z_test"
    );
    let syntax_links = program
        .syntax_configuration
        .language
        .relations()
        .iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let parser_link = &syntax_links[0];
    assert_eq!(
        parser_link.relation.as_deref(),
        Some("http://www.sap.com/adt/relations/abapsource/parser")
    );
    assert_eq!(parser_link.media_type.as_deref(), Some("text/plain"));
    assert_eq!(parser_link.title.as_deref(), Some("Standard ABAP"));
    assert_eq!(parser_link.etag.as_deref(), Some("757"));
    assert_eq!(
        program.source.uri.as_str(),
        "/sap/bc/adt/programs/programs/z_test/source/main"
    );
    assert_eq!(program.source.object, reference.erase());
    assert!(program.source.query.is_empty());
    assert_eq!(program.source.etag.as_deref(), Some("202607251959580001"));
    assert_eq!(program.etag.as_deref(), Some("202607251959580008"));
    let html_source = program.html_source().unwrap().unwrap();
    assert_eq!(html_source.object, reference.erase());
    assert_eq!(html_source.uri, program.source.uri);
    assert_eq!(
        program.versions().unwrap().unwrap().uri.as_str(),
        "/sap/bc/adt/programs/programs/z_test/source/main/versions"
    );
    assert_eq!(
        program.object_structure().unwrap().unwrap().uri.as_str(),
        "/sap/bc/adt/programs/programs/z_test/objectstructure"
    );
    assert_eq!(
        program.text_elements().unwrap().unwrap().uri.as_str(),
        "/sap/bc/adt/textelements/programs/z_test"
    );
    assert_eq!(
        program
            .enhancement_implementations()
            .unwrap()
            .unwrap()
            .uri
            .as_str(),
        "/sap/bc/adt/programs/programs/z_test/enhancements/implementations"
    );
    assert_eq!(
        program.enhancement_options().unwrap().unwrap().uri.as_str(),
        "/sap/bc/adt/programs/programs/z_test/enhancements/options"
    );
    assert_eq!(
        program
            .source_enhancement_options()
            .unwrap()
            .unwrap()
            .uri
            .as_str(),
        "/sap/bc/adt/programs/programs/z_test/source/main/enhancements/options"
    );
    assert_eq!(
        program.object_state().unwrap().unwrap().query,
        [("version".to_owned(), "active".to_owned())]
    );
    assert_eq!(
        program
            .syntax_configuration
            .language
            .parser()
            .unwrap()
            .unwrap()
            .uri
            .as_str(),
        "/sap/bc/adt/abapsource/parsers/rnd/grammar"
    );
    assert_eq!(source.content, SOURCE);

    logon.assert_async().await;
    discovery.assert_async().await;
    metadata.assert_async().await;
    get_source.assert_async().await;
}

#[tokio::test]
async fn program_properties_query_honors_v2_first_priority() {
    let server = MockServer::start_async().await;
    let logon = mock_logon(&server).await;
    let _core_discovery = mock_core_discovery(&server).await;
    let discovery = server
        .mock_async(|when, then| {
            when.method(GET).path("/sap/bc/adt/discovery");
            then.status(200).body(DISCOVERY_XML);
        })
        .await;
    let metadata = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/programs/programs/z_test")
                .query_param("version", "workingArea")
                .header("accept", "application/vnd.sap.adt.programs.programs.v2+xml");
            then.status(200)
                .header(
                    "content-type",
                    "application/vnd.sap.adt.programs.programs.v2+xml; charset=utf-8",
                )
                .header("etag", "202607251959580008")
                .body(PROGRAM_XML);
        })
        .await;
    let transport = ReqwestTransport::builder()
        .destination(server.base_url())
        .sap_client("001")
        .language("EN")
        .basic_auth("USER", "PASSWORD")
        .build()
        .unwrap();

    let client = ready_client(transport).await;
    let response = client
        .object::<Program>("Z_TEST")
        .unwrap()
        .query()
        .priority([ProgramPropertiesVersion::V2, ProgramPropertiesVersion::V3])
        .version(ObjectVersion::WorkingArea)
        .execute(&client)
        .await
        .unwrap();
    assert_eq!(response.media_version(), ProgramPropertiesVersion::V2);
    let program = match response {
        ProgramProperties::V2(program) => *program,
        _ => panic!("unexpected program-properties version"),
    };

    assert_eq!(program.name, "Z_TEST");
    assert_eq!(program.version, ObjectVersion::Inactive);
    assert_eq!(program.source.etag.as_deref(), Some("202607251959580001"));
    logon.assert_async().await;
    discovery.assert_async().await;
    metadata.assert_async().await;
}

#[tokio::test]
async fn program_properties_query_returns_not_modified_for_a_current_etag() {
    let server = MockServer::start_async().await;
    let logon = mock_logon(&server).await;
    let _core_discovery = mock_core_discovery(&server).await;
    let discovery = server
        .mock_async(|when, then| {
            when.method(GET).path("/sap/bc/adt/discovery");
            then.status(200).body(DISCOVERY_XML);
        })
        .await;
    let metadata = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/programs/programs/z_test")
                .query_param("version", "inactive")
                .header("accept", "application/vnd.sap.adt.programs.programs.v3+xml")
                .header("if-none-match", "202607251959580008");
            then.status(304).header("etag", "202607251959580008");
        })
        .await;
    let transport = ReqwestTransport::builder()
        .destination(server.base_url())
        .sap_client("001")
        .language("EN")
        .basic_auth("USER", "PASSWORD")
        .build()
        .unwrap();

    let client = ready_client(transport).await;
    let response = client
        .object::<Program>("Z_TEST")
        .unwrap()
        .query()
        .version(ObjectVersion::Inactive)
        .if_none_match(EntityTag::from_static("202607251959580008"))
        .execute(&client)
        .await
        .unwrap();

    assert!(matches!(
        &response,
        Revalidation::NotModified {
            etag: Some(etag)
        } if etag == "202607251959580008"
    ));
    assert_eq!(response.not_modified_etag(), Some("202607251959580008"));
    assert!(response.as_modified().is_none());
    logon.assert_async().await;
    discovery.assert_async().await;
    metadata.assert_async().await;
}

#[tokio::test]
async fn program_lock_and_update_share_one_user_session() {
    let server = MockServer::start_async().await;
    let logon = mock_logon(&server).await;
    let _core_discovery = mock_core_discovery(&server).await;
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
                .header("x-sap-adt-sessiontype", "stateless")
                .header("cookie", "sap-usercontext=sap-client=001&sap-language=EN");
            then.status(200).header("x-csrf-token", "CSRF-TOKEN-1");
        })
        .await;
    let get_source = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/sap/bc/adt/programs/programs/z_ziege_test/source/main")
                .header("accept", "text/plain");
            then.status(200)
                .header("etag", "SOURCE-ETAG-1")
                .body(SOURCE);
        })
        .await;
    let lock_program = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/sap/bc/adt/programs/programs/z_ziege_test")
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
                .path("/sap/bc/adt/programs/programs/z_ziege_test/source/main")
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
                .path("/sap/bc/adt/programs/programs/z_ziege_test")
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

    let client = ready_client(transport).await;
    let program = client.object::<Program>("Z_ZIEGE_TEST").unwrap();
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
    assert_eq!(lock_handle.object().uri(), program.uri());
    assert_eq!(lock_handle.handle(), "LOCK-HANDLE-1");
    program
        .unlock(lock_handle)
        .unwrap()
        .execute(&session)
        .await
        .unwrap();
    session.close().await.unwrap();

    assert_eq!(source.etag.as_deref(), Some("SOURCE-ETAG-1"));
    logon.assert_async().await;
    discovery.assert_async().await;
    csrf.assert_async().await;
    get_source.assert_async().await;
    lock_program.assert_async().await;
    update_source.assert_async().await;
    unlock_program.assert_async().await;
    close_session.assert_async().await;
}
