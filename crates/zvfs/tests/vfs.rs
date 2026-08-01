use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use http::{HeaderMap, StatusCode};
use zadt::{
    AdtRequest, AdtResponse, Client, RepositoryFacet, RepositoryPreselection, Transport,
    TransportError,
};
use zvfs::{FacetPolicy, Mount, NodeId, NodeKind, RepositoryVfs, VfsError};

const DISCOVERY_XML: &str = include_str!("../../zadt/tests/fixtures/discovery.xml");

const EMPTY_XML: &str = r#"
    <vfs:virtualFoldersResult xmlns:vfs="http://www.sap.com/adt/ris/virtualFolders"
        objectCount="0" />
"#;

const CHILD_PACKAGES_XML: &str = r#"
    <vfs:virtualFoldersResult xmlns:vfs="http://www.sap.com/adt/ris/virtualFolders"
        objectCount="7">
        <vfs:virtualFolder name="/ROOT/CHILD" displayName="Child Package" facet="PACKAGE"
            counter="7" hasChildrenOfSameFacet="false" />
    </vfs:virtualFoldersResult>
"#;

const GROUP_XML: &str = r#"
    <vfs:virtualFoldersResult xmlns:vfs="http://www.sap.com/adt/ris/virtualFolders"
        objectCount="12">
        <vfs:virtualFolder name="SOURCE_LIBRARY" displayName="Source Code Library" facet="GROUP"
            counter="12" hasChildrenOfSameFacet="false" />
    </vfs:virtualFoldersResult>
"#;

const TYPE_XML: &str = r#"
    <vfs:virtualFoldersResult xmlns:vfs="http://www.sap.com/adt/ris/virtualFolders"
        objectCount="12">
        <vfs:virtualFolder name="CLAS" displayName="Classes" facet="TYPE"
            counter="12" hasChildrenOfSameFacet="false" />
    </vfs:virtualFoldersResult>
"#;

const OBJECT_XML: &str = r#"
    <vfs:virtualFoldersResult xmlns:vfs="http://www.sap.com/adt/ris/virtualFolders"
        objectCount="1">
        <vfs:object name="ZCL_DEMO" package="/ROOT" type="CLAS/OC"
            uri="/sap/bc/adt/oo/classes/zcl_demo" expandable="true" text="Demo class" />
    </vfs:virtualFoldersResult>
"#;

#[derive(Clone, Copy)]
enum Behavior {
    Tree,
    Adaptive(u32),
    AdaptiveRefresh,
    Hierarchical,
    SlowEmpty,
    FailOnce,
    FailRefresh,
    Refresh,
}

#[derive(Clone)]
struct TestTransport {
    behavior: Behavior,
    state: Arc<TransportState>,
}

#[derive(Default)]
struct TransportState {
    requests: Mutex<Vec<String>>,
    post_count: AtomicUsize,
    active: AtomicUsize,
    max_active: AtomicUsize,
}

impl TestTransport {
    fn new(behavior: Behavior) -> (Self, Arc<TransportState>) {
        let state = Arc::new(TransportState::default());
        (
            Self {
                behavior,
                state: state.clone(),
            },
            state,
        )
    }

    fn response(body: impl Into<Vec<u8>>) -> AdtResponse {
        AdtResponse::new(StatusCode::OK, HeaderMap::new(), body.into())
    }

    fn repository_response(&self, body: &str, request_number: usize) -> Result<String, io::Error> {
        match self.behavior {
            Behavior::Tree => {
                if body.contains("<vfs:value>/ROOT</vfs:value>")
                    && body.contains("<vfs:facet>PACKAGE</vfs:facet>")
                {
                    Ok(CHILD_PACKAGES_XML.to_owned())
                } else if body.contains("<vfs:value>../ROOT</vfs:value>")
                    && body.contains("<vfs:facet>GROUP</vfs:facet>")
                {
                    Ok(GROUP_XML.to_owned())
                } else if body.contains("<vfs:value>SOURCE_LIBRARY</vfs:value>")
                    && body.contains("<vfs:facet>TYPE</vfs:facet>")
                {
                    Ok(TYPE_XML.to_owned())
                } else if body.contains("<vfs:value>CLAS</vfs:value>")
                    && !body.contains("<vfs:facet>TYPE</vfs:facet>")
                {
                    Ok(OBJECT_XML.to_owned())
                } else {
                    Err(io::Error::other(format!(
                        "unexpected tree request body: {body}"
                    )))
                }
            }
            Behavior::Adaptive(count) => {
                if body.contains("<vfs:facet>GROUP</vfs:facet>") {
                    Ok(format!(
                        r#"<vfs:virtualFoldersResult xmlns:vfs="http://www.sap.com/adt/ris/virtualFolders" objectCount="{count}">
                            <vfs:virtualFolder name="SOURCE_LIBRARY" displayName="Source Code Library"
                                facet="GROUP" counter="{count}" hasChildrenOfSameFacet="false" />
                        </vfs:virtualFoldersResult>"#
                    ))
                } else {
                    Ok(OBJECT_XML.to_owned())
                }
            }
            Behavior::AdaptiveRefresh => match request_number {
                0 => Ok(GROUP_XML
                    .replace("objectCount=\"12\"", "objectCount=\"30\"")
                    .replace("counter=\"12\"", "counter=\"30\"")),
                1 => Ok(TYPE_XML
                    .replace("objectCount=\"12\"", "objectCount=\"30\"")
                    .replace("counter=\"12\"", "counter=\"30\"")),
                2 => Ok(TYPE_XML
                    .replace("objectCount=\"12\"", "objectCount=\"3\"")
                    .replace("counter=\"12\"", "counter=\"3\"")),
                _ => Ok(OBJECT_XML.replace("objectCount=\"1\"", "objectCount=\"3\"")),
            },
            Behavior::Hierarchical => {
                if body.contains("<vfs:facet>TYPE</vfs:facet>") {
                    Ok(TYPE_XML.to_owned())
                } else if body.contains("<vfs:value>LEAF_APPL</vfs:value>") {
                    Ok(r#"<vfs:virtualFoldersResult xmlns:vfs="http://www.sap.com/adt/ris/virtualFolders" objectCount="2">
                            <vfs:virtualFolder name="NEW_SUB_APPL" displayName="New Subcomponent" facet="APPL"
                                counter="2" hasChildrenOfSameFacet="false" />
                        </vfs:virtualFoldersResult>"#
                        .to_owned())
                } else if body.contains("<vfs:value>ROOT_APPL</vfs:value>") {
                    Ok(r#"<vfs:virtualFoldersResult xmlns:vfs="http://www.sap.com/adt/ris/virtualFolders" objectCount="4">
                            <vfs:virtualFolder name="LEAF_APPL" displayName="Leaf Component" facet="APPL"
                                counter="4" hasChildrenOfSameFacet="false" />
                        </vfs:virtualFoldersResult>"#
                        .to_owned())
                } else {
                    Ok(r#"<vfs:virtualFoldersResult xmlns:vfs="http://www.sap.com/adt/ris/virtualFolders" objectCount="8">
                            <vfs:virtualFolder name="ROOT_APPL" displayName="Root Component" facet="APPL"
                                counter="8" hasChildrenOfSameFacet="true" />
                        </vfs:virtualFoldersResult>"#
                        .to_owned())
                }
            }
            Behavior::SlowEmpty => Ok(EMPTY_XML.to_owned()),
            Behavior::FailOnce if request_number == 0 => {
                Err(io::Error::other("temporary repository failure"))
            }
            Behavior::FailOnce => Ok(EMPTY_XML.to_owned()),
            Behavior::FailRefresh if request_number > 0 => {
                Err(io::Error::other("temporary refresh failure"))
            }
            Behavior::FailRefresh => Ok(OBJECT_XML.to_owned()),
            Behavior::Refresh => {
                let name = if request_number == 0 {
                    "Z_FIRST"
                } else {
                    "Z_SECOND"
                };
                Ok(format!(
                    r#"<vfs:virtualFoldersResult xmlns:vfs="http://www.sap.com/adt/ris/virtualFolders" objectCount="1">
                        <vfs:object name="{name}" package="$TMP" type="PROG/P"
                            uri="/sap/bc/adt/programs/programs/{name}" expandable="true" />
                    </vfs:virtualFoldersResult>"#
                ))
            }
        }
    }
}

#[async_trait]
impl Transport for TestTransport {
    async fn send(&self, request: AdtRequest) -> Result<AdtResponse, TransportError> {
        if request.target().as_str() == "/sap/bc/adt/discovery" {
            return Ok(Self::response(DISCOVERY_XML.as_bytes().to_vec()));
        }

        let body = String::from_utf8_lossy(request.body()).into_owned();
        self.state.requests.lock().unwrap().push(body.clone());
        let request_number = self.state.post_count.fetch_add(1, Ordering::SeqCst);

        if matches!(self.behavior, Behavior::SlowEmpty) {
            let active = self.state.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.state.max_active.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(40)).await;
            self.state.active.fetch_sub(1, Ordering::SeqCst);
        }

        let response = self
            .repository_response(&body, request_number)
            .map_err(TransportError::new)?;
        Ok(Self::response(response.into_bytes()))
    }
}

async fn client(behavior: Behavior) -> (zadt::Client<zadt::Ready>, Arc<TransportState>) {
    let (transport, state) = TestTransport::new(behavior);
    let client = Client::new(transport).discover().await.unwrap();
    (client, state)
}

fn selection_mount(label: &str) -> Mount {
    Mount::selection(
        label,
        [RepositoryPreselection::new(
            RepositoryFacet::OWNER,
            "DEVELOPER",
        )],
    )
}

#[tokio::test]
async fn traverses_packages_groups_types_and_objects() {
    let (client, state) = client(Behavior::Tree).await;
    let vfs = RepositoryVfs::builder(client)
        .mount(Mount::package("/ROOT"))
        .build();

    let mounts = vfs.children(vfs.root()).await.unwrap();
    assert_eq!(mounts.len(), 1);
    assert!(matches!(mounts[0].kind, NodeKind::Package { .. }));

    let package_children = vfs.children(mounts[0].id).await.unwrap();
    assert_eq!(
        package_children
            .iter()
            .map(|node| node.label.as_str())
            .collect::<Vec<_>>(),
        ["Child Package", "Source Code Library"]
    );

    let group = package_children
        .iter()
        .find(|node| node.label == "Source Code Library")
        .unwrap();
    let types = vfs.children(group.id).await.unwrap();
    assert_eq!(types[0].label, "Classes");

    let objects = vfs.children(types[0].id).await.unwrap();
    assert_eq!(objects[0].label, "ZCL_DEMO");
    assert!(!objects[0].is_directory());
    assert_eq!(
        vfs.object_entry(objects[0].id)
            .unwrap()
            .reference
            .uri()
            .as_str(),
        "/sap/bc/adt/oo/classes/zcl_demo"
    );

    let path = vfs.path(objects[0].id).unwrap();
    assert_eq!(
        path.iter()
            .map(|node| node.label.as_str())
            .collect::<Vec<_>>(),
        ["/", "/ROOT", "Source Code Library", "Classes", "ZCL_DEMO"]
    );

    let json = serde_json::to_string(&objects[0]).unwrap();
    let decoded: zvfs::Node = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, objects[0]);
    let id_json = serde_json::to_string(&objects[0].id).unwrap();
    let decoded_id: NodeId = serde_json::from_str(&id_json).unwrap();
    assert_eq!(decoded_id, objects[0].id);

    let requests = state.requests.lock().unwrap();
    assert!(
        requests
            .iter()
            .any(|request| request.contains("<vfs:value>../ROOT</vfs:value>"))
    );
}

#[tokio::test]
async fn adaptive_facets_flatten_small_layers_and_keep_large_layers() {
    for (count, expected_label, expected_requests) in
        [(3, "ZCL_DEMO", 2), (30, "Source Code Library", 1)]
    {
        let (client, state) = client(Behavior::Adaptive(count)).await;
        let vfs = RepositoryVfs::builder(client)
            .mount(selection_mount("Objects"))
            .facet_policy(FacetPolicy::adaptive(
                10,
                [RepositoryFacet::GROUP, RepositoryFacet::TYPE],
            ))
            .build();
        let mount = vfs.children(vfs.root()).await.unwrap().remove(0);

        let children = vfs.children(mount.id).await.unwrap();

        assert_eq!(children[0].label, expected_label);
        assert_eq!(state.post_count.load(Ordering::SeqCst), expected_requests);
    }
}

#[tokio::test]
async fn repeats_hierarchical_facets_before_advancing() {
    let (client, state) = client(Behavior::Hierarchical).await;
    let vfs = RepositoryVfs::builder(client)
        .mount(selection_mount("Objects"))
        .facet_policy(FacetPolicy::grouped([
            RepositoryFacet::APPLICATION_COMPONENT,
            RepositoryFacet::TYPE,
        ]))
        .build();
    let mount = vfs.children(vfs.root()).await.unwrap().remove(0);

    let root_component = vfs.children(mount.id).await.unwrap().remove(0);
    let leaf_component = vfs.children(root_component.id).await.unwrap().remove(0);
    let object_types = vfs.children(leaf_component.id).await.unwrap();

    assert_eq!(root_component.label, "Root Component");
    assert_eq!(leaf_component.label, "Leaf Component");
    assert_eq!(object_types[0].label, "Classes");
    {
        let requests = state.requests.lock().unwrap();
        assert!(requests[1].contains("<vfs:facet>APPL</vfs:facet>"));
        assert!(requests[2].contains("<vfs:facet>TYPE</vfs:facet>"));
    }

    let refreshed = vfs.refresh(leaf_component.id).await.unwrap();

    assert_eq!(refreshed[0].label, "New Subcomponent");
    assert!(matches!(
        vfs.node(leaf_component.id).unwrap().kind,
        NodeKind::Facet {
            has_children_of_same_facet: true,
            ..
        }
    ));
    let requests = state.requests.lock().unwrap();
    assert!(requests[3].contains("<vfs:facet>APPL</vfs:facet>"));
}

#[tokio::test]
async fn adaptive_refresh_rechecks_the_current_object_count() {
    let (client, _) = client(Behavior::AdaptiveRefresh).await;
    let vfs = RepositoryVfs::builder(client)
        .mount(selection_mount("Objects"))
        .facet_policy(FacetPolicy::adaptive(
            10,
            [RepositoryFacet::GROUP, RepositoryFacet::TYPE],
        ))
        .build();
    let mount = vfs.children(vfs.root()).await.unwrap().remove(0);
    let group = vfs.children(mount.id).await.unwrap().remove(0);
    let old_type = vfs.children(group.id).await.unwrap().remove(0);

    let refreshed = vfs.refresh(group.id).await.unwrap();

    assert_eq!(refreshed[0].label, "ZCL_DEMO");
    assert!(vfs.node(old_type.id).is_none());
    assert!(matches!(
        vfs.node(group.id).unwrap().kind,
        NodeKind::Facet {
            object_count: 3,
            ..
        }
    ));
}

#[tokio::test]
async fn scopes_loading_locks_to_individual_nodes() {
    let (client, state) = client(Behavior::SlowEmpty).await;
    let vfs = RepositoryVfs::builder(client)
        .mount(selection_mount("First"))
        .mount(selection_mount("Second"))
        .facet_policy(FacetPolicy::Flat)
        .build();
    let mounts = vfs.children(vfs.root()).await.unwrap();

    let (first, second) = tokio::join!(vfs.children(mounts[0].id), vfs.children(mounts[1].id));

    first.unwrap();
    second.unwrap();
    assert_eq!(state.post_count.load(Ordering::SeqCst), 2);
    assert_eq!(state.max_active.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn deduplicates_concurrent_loads_of_the_same_node() {
    let (client, state) = client(Behavior::SlowEmpty).await;
    let vfs = RepositoryVfs::builder(client)
        .mount(selection_mount("Objects"))
        .facet_policy(FacetPolicy::Flat)
        .build();
    let mount = vfs.children(vfs.root()).await.unwrap().remove(0);

    let (first, second) = tokio::join!(vfs.children(mount.id), vfs.children(mount.id));

    assert!(first.unwrap().is_empty());
    assert!(second.unwrap().is_empty());
    assert_eq!(state.post_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn retries_failed_expansions_instead_of_caching_the_error() {
    let (client, state) = client(Behavior::FailOnce).await;
    let vfs = RepositoryVfs::builder(client)
        .mount(selection_mount("Objects"))
        .facet_policy(FacetPolicy::Flat)
        .build();
    let mount = vfs.children(vfs.root()).await.unwrap().remove(0);

    assert!(vfs.children(mount.id).await.is_err());
    assert!(vfs.children(mount.id).await.unwrap().is_empty());
    assert_eq!(state.post_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn refresh_replaces_descendants_and_invalidates_old_ids() {
    let (client, _) = client(Behavior::Refresh).await;
    let vfs = RepositoryVfs::builder(client)
        .mount(selection_mount("Objects"))
        .facet_policy(FacetPolicy::Flat)
        .build();
    let mount = vfs.children(vfs.root()).await.unwrap().remove(0);
    let first = vfs.children(mount.id).await.unwrap().remove(0);

    let second = vfs.refresh(mount.id).await.unwrap().remove(0);

    assert_eq!(first.label, "Z_FIRST");
    assert_eq!(second.label, "Z_SECOND");
    assert!(vfs.node(first.id).is_none());
    assert!(matches!(
        vfs.object_entry(first.id),
        Err(VfsError::UnknownNode(id)) if id == first.id
    ));
}

#[tokio::test]
async fn failed_refresh_preserves_the_cached_subtree() {
    let (client, _) = client(Behavior::FailRefresh).await;
    let vfs = RepositoryVfs::builder(client)
        .mount(selection_mount("Objects"))
        .facet_policy(FacetPolicy::Flat)
        .build();
    let mount = vfs.children(vfs.root()).await.unwrap().remove(0);
    let object = vfs.children(mount.id).await.unwrap().remove(0);

    assert!(vfs.refresh(mount.id).await.is_err());

    let cached = vfs.cached_children(mount.id).unwrap().unwrap();
    assert_eq!(cached.as_slice(), std::slice::from_ref(&object));
    assert_eq!(vfs.node(object.id), Some(object));
}

#[tokio::test]
async fn rejects_node_ids_from_another_vfs_instance() {
    let (first_client, _) = client(Behavior::SlowEmpty).await;
    let (second_client, _) = client(Behavior::SlowEmpty).await;
    let first = RepositoryVfs::builder(first_client).build();
    let second = RepositoryVfs::builder(second_client).build();

    assert_ne!(first.root(), second.root());
    assert!(first.node(second.root()).is_none());
    assert!(matches!(
        first.children(second.root()).await,
        Err(VfsError::UnknownNode(id)) if id == second.root()
    ));
}
