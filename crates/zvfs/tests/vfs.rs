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
use tokio::sync::Notify;
use zadt::{
    AdtRequest, AdtResponse, Client, RepositoryFacet, RepositoryPreselection, Transport,
    TransportError,
};
use zvfs::{FacetLevel, FacetPolicy, Mount, NodeId, NodeKind, VfsError, VirtualRepositoryTree};

const DISCOVERY_XML: &str = include_str!("../../zadt/tests/fixtures/discovery.xml");

const FACETS_XML: &str = r#"
    <vf:facets xmlns:vf="http://www.sap.com/adt/ris/facets">
        <vf:facet key="package" displayName="Package" description="Package"
            isHierarchical="true" isForFiltering="true" isForStructuring="true" />
        <vf:facet key="appl" displayName="Application Component" description="Application Component"
            isHierarchical="true" isForFiltering="true" isForStructuring="true" />
        <vf:facet key="owner" displayName="Owner" description="Owner"
            isHierarchical="false" isForFiltering="true" isForStructuring="true" />
        <vf:facet key="group" displayName="Group" description="Group"
            isHierarchical="false" isForFiltering="true" isForStructuring="true" />
        <vf:facet key="type" displayName="Type" description="Type"
            isHierarchical="false" isForFiltering="true" isForStructuring="true" />
        <vf:facet key="filter_only" displayName="Filter Only" description="Filter Only"
            isHierarchical="false" isForFiltering="true" isForStructuring="false" />
    </vf:facets>
"#;

const EMPTY_XML: &str = r#"
    <vfs:virtualFoldersResult xmlns:vfs="http://www.sap.com/adt/ris/virtualFolders"
        objectCount="0" />
"#;

const CHILD_PACKAGES_XML: &str = r#"
    <vfs:virtualFoldersResult xmlns:vfs="http://www.sap.com/adt/ris/virtualFolders"
        objectCount="9">
        <vfs:preselectionInfo facet="PACKAGE" hasChildrenOfSameFacet="true" />
        <vfs:virtualFolder name="../ROOT" displayName="../ROOT" facet="PACKAGE"
            counter="2" hasChildrenOfSameFacet="false" />
        <vfs:virtualFolder name="/ROOT/CHILD" displayName="/ROOT/CHILD" facet="PACKAGE"
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

const OWNER_XML: &str = r#"
    <vfs:virtualFoldersResult xmlns:vfs="http://www.sap.com/adt/ris/virtualFolders"
        objectCount="12">
        <vfs:virtualFolder name="DEVELOPER" displayName="DEVELOPER" facet="OWNER"
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
    AdaptiveHierarchyRefresh,
    Propagation,
    Hierarchical,
    SlowEmpty,
    FailOnce,
    FailRefresh,
    Refresh,
    AncestorRefreshRace,
}

#[derive(Clone)]
struct TestTransport {
    behavior: Behavior,
    state: Arc<TransportState>,
}

#[derive(Default)]
struct TransportState {
    requests: Mutex<Vec<String>>,
    facet_count: AtomicUsize,
    post_count: AtomicUsize,
    active: AtomicUsize,
    max_active: AtomicUsize,
    descendant_started: Notify,
    release_descendant: Notify,
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
                } else if body.contains("<vfs:facet>PACKAGE</vfs:facet>") {
                    Ok(EMPTY_XML.to_owned())
                } else if body.contains("<vfs:facet>OWNER</vfs:facet>") {
                    Ok(OWNER_XML.to_owned())
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
                } else if body.contains("<vfs:facet>TYPE</vfs:facet>") {
                    Ok(TYPE_XML
                        .replace("objectCount=\"12\"", &format!("objectCount=\"{count}\""))
                        .replace("counter=\"12\"", &format!("counter=\"{count}\"")))
                } else {
                    Ok(
                        OBJECT_XML
                            .replace("objectCount=\"1\"", &format!("objectCount=\"{count}\"")),
                    )
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
            Behavior::AdaptiveHierarchyRefresh => match request_number {
                0 => Ok(r#"<vfs:virtualFoldersResult xmlns:vfs="http://www.sap.com/adt/ris/virtualFolders" objectCount="20">
                        <vfs:virtualFolder name="ROOT_APPL" displayName="Root Component" facet="APPL"
                            counter="20" hasChildrenOfSameFacet="true" />
                    </vfs:virtualFoldersResult>"#
                    .to_owned()),
                1 => Ok(r#"<vfs:virtualFoldersResult xmlns:vfs="http://www.sap.com/adt/ris/virtualFolders" objectCount="20">
                        <vfs:virtualFolder name="LEAF_APPL" displayName="Leaf Component" facet="APPL"
                            counter="20" hasChildrenOfSameFacet="false" />
                    </vfs:virtualFoldersResult>"#
                    .to_owned()),
                2 => Ok(r#"<vfs:virtualFoldersResult xmlns:vfs="http://www.sap.com/adt/ris/virtualFolders" objectCount="3">
                        <vfs:virtualFolder name="LEAF_APPL" displayName="Leaf Component" facet="APPL"
                            counter="3" hasChildrenOfSameFacet="false" />
                    </vfs:virtualFoldersResult>"#
                    .to_owned()),
                _ => Ok(TYPE_XML
                    .replace("objectCount=\"12\"", "objectCount=\"3\"")
                    .replace("counter=\"12\"", "counter=\"3\"")),
            },
            Behavior::Propagation => {
                if body.contains("<vfs:facet>OWNER</vfs:facet>") {
                    Ok(OWNER_XML.to_owned())
                } else if body.contains("<vfs:facet>GROUP</vfs:facet>") {
                    Ok(GROUP_XML.to_owned())
                } else if body.contains("<vfs:facet>TYPE</vfs:facet>") {
                    Ok(TYPE_XML.to_owned())
                } else {
                    Ok(OBJECT_XML.to_owned())
                }
            }
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
            Behavior::AncestorRefreshRace => {
                if body.contains("<vfs:facet>GROUP</vfs:facet>") {
                    Ok(GROUP_XML.to_owned())
                } else if body.contains("<vfs:value>SOURCE_LIBRARY</vfs:value>") {
                    Ok(OBJECT_XML.to_owned())
                } else {
                    Err(io::Error::other(format!(
                        "unexpected ancestor-refresh race request body: {body}"
                    )))
                }
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
        if request.target().as_str()
            == "/sap/bc/adt/repository/informationsystem/virtualfolders/facets"
        {
            self.state.facet_count.fetch_add(1, Ordering::SeqCst);
            return Ok(Self::response(FACETS_XML.as_bytes().to_vec()));
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

        if matches!(self.behavior, Behavior::AncestorRefreshRace)
            && body.contains("<vfs:value>SOURCE_LIBRARY</vfs:value>")
            && !body.contains("<vfs:facet>")
        {
            self.state.descendant_started.notify_one();
            self.state.release_descendant.notified().await;
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

fn flat_selection_mount(label: &str) -> Mount {
    selection_mount(label).facet_policy(FacetPolicy::flat())
}

fn preselection_blocks<'a>(body: &'a str, facet: &str) -> Vec<&'a str> {
    let marker = format!("<vfs:preselection facet=\"{facet}\">");
    let end_marker = "</vfs:preselection>";
    let mut blocks = Vec::new();
    let mut remainder = body;

    while let Some(start) = remainder.find(&marker) {
        remainder = &remainder[start..];
        let end = remainder
            .find(end_marker)
            .expect("serialized preselections are closed")
            + end_marker.len();
        blocks.push(&remainder[..end]);
        remainder = &remainder[end..];
    }

    blocks
}

fn assert_preselection(body: &str, facet: &str, values: &[&str]) {
    let blocks = preselection_blocks(body, facet);
    assert!(
        blocks.iter().any(|block| values
            .iter()
            .all(|value| { block.contains(&format!("<vfs:value>{value}</vfs:value>")) })),
        "missing {facet} preselection with {values:?} in {body}"
    );
}

fn assert_exact_preselection(body: &str, facet: &str, values: &[&str]) {
    let blocks = preselection_blocks(body, facet);
    assert!(
        blocks.iter().any(|block| {
            block.matches("<vfs:value>").count() == values.len()
                && values
                    .iter()
                    .all(|value| block.contains(&format!("<vfs:value>{value}</vfs:value>")))
        }),
        "missing exact {facet} preselection with {values:?} in {body}"
    );
}

fn assert_output_facet(body: &str, facet: Option<&str>) {
    if let Some(facet) = facet {
        assert!(body.contains(&format!("<vfs:facet>{facet}</vfs:facet>")));
    } else {
        assert!(!body.contains("<vfs:facet>"));
    }
}

fn successor(id: NodeId) -> NodeId {
    let mut value = serde_json::to_value(id).unwrap();
    let index = value["index"].as_u64().unwrap();
    value["index"] = serde_json::Value::from(index.checked_add(1).unwrap());
    serde_json::from_value(value).unwrap()
}

#[tokio::test]
async fn validates_facet_policies_while_building() {
    let missing = RepositoryFacet::from("MISSING");
    let (missing_client, state) = client(Behavior::SlowEmpty).await;
    let result = VirtualRepositoryTree::builder(missing_client)
        .mount(selection_mount("Missing").facet_policy(FacetPolicy::grouped([missing.clone()])))
        .build()
        .await;

    assert!(matches!(
        result,
        Err(VfsError::UnsupportedFacet(facet)) if facet == missing
    ));
    assert_eq!(state.facet_count.load(Ordering::SeqCst), 1);

    let unstructured = RepositoryFacet::from("FILTER_ONLY");
    let (unstructured_client, _) = client(Behavior::SlowEmpty).await;
    let result = VirtualRepositoryTree::builder(unstructured_client)
        .mount(
            selection_mount("Unstructured")
                .facet_policy(FacetPolicy::grouped([unstructured.clone()])),
        )
        .build()
        .await;

    assert!(matches!(
        result,
        Err(VfsError::UnstructuredFacet(facet)) if facet == unstructured
    ));
}

#[tokio::test]
async fn traverses_packages_groups_types_and_objects() {
    let (client, state) = client(Behavior::Tree).await;
    let vfs = VirtualRepositoryTree::builder(client)
        .mount(Mount::package("/ROOT"))
        .build()
        .await
        .unwrap();

    assert_eq!(state.facet_count.load(Ordering::SeqCst), 1);

    let mounts = vfs.children(vfs.root()).await.unwrap();
    assert_eq!(mounts.len(), 1);
    assert!(matches!(mounts[0].kind, NodeKind::Package { .. }));

    let package_children = vfs.children(mounts[0].id).await.unwrap();
    assert_eq!(
        package_children
            .iter()
            .map(|node| node.label.as_str())
            .collect::<Vec<_>>(),
        ["/ROOT/CHILD", "Source Code Library"]
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

    assert_eq!(
        vfs.render_tree(),
        "/\n└── /ROOT\n    ├── /ROOT/CHILD\n    └── Source Code Library\n        └── Classes\n            └── ZCL_DEMO"
    );

    let requests = state.requests.lock().unwrap();
    assert!(
        requests
            .iter()
            .any(|request| request.contains("<vfs:value>../ROOT</vfs:value>"))
    );
}

#[tokio::test]
async fn child_package_metadata_controls_hierarchy_queries() {
    let (client, state) = client(Behavior::Tree).await;
    let vfs = VirtualRepositoryTree::builder(client)
        .mount(Mount::package("/ROOT").facet_policy(FacetPolicy::grouped([RepositoryFacet::OWNER])))
        .build()
        .await
        .unwrap();
    let package = vfs.children(vfs.root()).await.unwrap().remove(0);
    let children = vfs.children(package.id).await.unwrap();
    let child_package = children
        .iter()
        .find(|node| node.label == "/ROOT/CHILD")
        .unwrap();

    let child_contents = vfs.children(child_package.id).await.unwrap();

    assert_eq!(
        child_contents
            .iter()
            .map(|node| node.label.as_str())
            .collect::<Vec<_>>(),
        ["DEVELOPER"]
    );
    {
        let requests = state.requests.lock().unwrap();
        let root_direct = requests
            .iter()
            .find(|request| request.contains("<vfs:value>../ROOT</vfs:value>"))
            .unwrap();
        assert_output_facet(root_direct, Some("OWNER"));
        let child_direct = requests
            .iter()
            .find(|request| request.contains("<vfs:value>../ROOT/CHILD</vfs:value>"))
            .unwrap();
        assert_output_facet(child_direct, Some("OWNER"));
        assert!(
            !requests
                .iter()
                .any(|request| request.contains("<vfs:value>/ROOT/CHILD</vfs:value>"))
        );
    }

    vfs.refresh(child_package.id).await.unwrap();

    let requests = state.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.contains("<vfs:value>/ROOT/CHILD</vfs:value>"))
            .count(),
        1
    );
}

#[tokio::test]
async fn adaptive_type_facets_skip_small_layers_and_keep_large_layers() {
    for (count, expected_label, expected_requests) in [(3, "ZCL_DEMO", 3), (10, "Classes", 2)] {
        let (client, state) = client(Behavior::Adaptive(count)).await;
        let vfs = VirtualRepositoryTree::builder(client)
            .mount(selection_mount("Objects").facet_policy(FacetPolicy::new([
                FacetLevel::always(RepositoryFacet::GROUP),
                FacetLevel::adaptive(RepositoryFacet::TYPE, 10),
            ])))
            .build()
            .await
            .unwrap();
        let mount = vfs.children(vfs.root()).await.unwrap().remove(0);
        let group = vfs.children(mount.id).await.unwrap().remove(0);

        let children = vfs.children(group.id).await.unwrap();

        assert_eq!(children[0].label, expected_label);
        assert_eq!(state.post_count.load(Ordering::SeqCst), expected_requests);
    }
}

#[tokio::test]
async fn adaptive_facets_skip_only_their_own_level() {
    for (count, expected_label, expected_requests) in
        [(3, "Classes", 2), (10, "Source Code Library", 1)]
    {
        let (client, state) = client(Behavior::Adaptive(count)).await;
        let vfs = VirtualRepositoryTree::builder(client)
            .mount(selection_mount("Objects").facet_policy(FacetPolicy::new([
                FacetLevel::adaptive(RepositoryFacet::GROUP, 10),
                FacetLevel::always(RepositoryFacet::TYPE),
            ])))
            .build()
            .await
            .unwrap();
        let mount = vfs.children(vfs.root()).await.unwrap().remove(0);

        let children = vfs.children(mount.id).await.unwrap();

        assert_eq!(children[0].label, expected_label);
        assert_eq!(state.post_count.load(Ordering::SeqCst), expected_requests);
    }
}

#[tokio::test]
async fn applies_facet_policies_independently_per_mount() {
    let (client, state) = client(Behavior::Adaptive(12)).await;
    let group_mount = Mount::selection(
        "By Group",
        [RepositoryPreselection::new(
            RepositoryFacet::API_STATE,
            "GROUP_MOUNT",
        )],
    )
    .facet_policy(FacetPolicy::grouped([RepositoryFacet::GROUP]));
    let type_mount = Mount::selection(
        "By Type",
        [RepositoryPreselection::new(
            RepositoryFacet::API_STATE,
            "TYPE_MOUNT",
        )],
    )
    .facet_policy(FacetPolicy::grouped([RepositoryFacet::TYPE]));
    let vfs = VirtualRepositoryTree::builder(client)
        .mount(group_mount)
        .mount(type_mount)
        .build()
        .await
        .unwrap();
    let mounts = vfs.children(vfs.root()).await.unwrap();

    let groups = vfs.children(mounts[0].id).await.unwrap();
    let types = vfs.children(mounts[1].id).await.unwrap();

    assert_eq!(groups[0].label, "Source Code Library");
    assert_eq!(types[0].label, "Classes");
    let requests = state.requests.lock().unwrap();
    assert_preselection(&requests[0], "API", &["GROUP_MOUNT"]);
    assert_output_facet(&requests[0], Some("GROUP"));
    assert!(!requests[0].contains("TYPE_MOUNT"));
    assert_preselection(&requests[1], "API", &["TYPE_MOUNT"]);
    assert_output_facet(&requests[1], Some("TYPE"));
    assert!(!requests[1].contains("GROUP_MOUNT"));
}

#[tokio::test]
async fn carries_mount_and_selected_facet_filters_through_every_expansion() {
    let (client, state) = client(Behavior::Propagation).await;
    let mount = Mount::selection(
        "Local Favorites",
        [
            RepositoryPreselection::directly_assigned("$TMP"),
            RepositoryPreselection::new(RepositoryFacet::OWNER, "DEVELOPER").include("ALICE"),
            RepositoryPreselection::new(RepositoryFacet::FAVORITES, "$DEVELOPER"),
        ],
    )
    .facet_policy(FacetPolicy::grouped([
        RepositoryFacet::OWNER,
        RepositoryFacet::GROUP,
        RepositoryFacet::TYPE,
    ]));
    let vfs = VirtualRepositoryTree::builder(client)
        .mount(mount)
        .build()
        .await
        .unwrap();
    let mount = vfs.children(vfs.root()).await.unwrap().remove(0);

    let owner = vfs.children(mount.id).await.unwrap().remove(0);
    let group = vfs.children(owner.id).await.unwrap().remove(0);
    let object_type = vfs.children(group.id).await.unwrap().remove(0);
    let objects = vfs.children(object_type.id).await.unwrap();

    assert_eq!(objects[0].label, "ZCL_DEMO");
    let requests = state.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    for request in requests.iter() {
        assert_preselection(request, "PACKAGE", &["..$TMP"]);
        assert_preselection(request, "OWNER", &["DEVELOPER", "ALICE"]);
        assert_preselection(request, "FAV", &["$DEVELOPER"]);
    }
    assert_output_facet(&requests[0], Some("OWNER"));
    for request in &requests[1..] {
        assert_eq!(preselection_blocks(request, "OWNER").len(), 2);
        assert_exact_preselection(request, "OWNER", &["DEVELOPER"]);
    }
    assert_output_facet(&requests[1], Some("GROUP"));
    assert_preselection(&requests[2], "GROUP", &["SOURCE_LIBRARY"]);
    assert_output_facet(&requests[2], Some("TYPE"));
    assert_preselection(&requests[3], "GROUP", &["SOURCE_LIBRARY"]);
    assert_preselection(&requests[3], "TYPE", &["CLAS"]);
    assert_output_facet(&requests[3], None);
}

#[tokio::test]
async fn repeats_hierarchical_facets_before_advancing() {
    let (client, state) = client(Behavior::Hierarchical).await;
    let vfs = VirtualRepositoryTree::builder(client)
        .mount(
            selection_mount("Objects").facet_policy(FacetPolicy::grouped([
                RepositoryFacet::APPLICATION_COMPONENT,
                RepositoryFacet::TYPE,
            ])),
        )
        .build()
        .await
        .unwrap();
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
    for request in requests.iter() {
        assert_preselection(request, "OWNER", &["DEVELOPER"]);
    }
    assert_preselection(&requests[1], "APPL", &["ROOT_APPL"]);
    assert_preselection(&requests[2], "APPL", &["ROOT_APPL"]);
    assert_preselection(&requests[2], "APPL", &["LEAF_APPL"]);
    assert_preselection(&requests[3], "APPL", &["ROOT_APPL"]);
    assert_preselection(&requests[3], "APPL", &["LEAF_APPL"]);
}

#[tokio::test]
async fn adaptive_refresh_rechecks_the_current_object_count() {
    let (client, state) = client(Behavior::AdaptiveRefresh).await;
    let vfs = VirtualRepositoryTree::builder(client)
        .mount(selection_mount("Objects").facet_policy(FacetPolicy::new([
            FacetLevel::always(RepositoryFacet::GROUP),
            FacetLevel::adaptive(RepositoryFacet::TYPE, 10),
        ])))
        .build()
        .await
        .unwrap();
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
    let requests = state.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    for request in requests.iter() {
        assert_preselection(request, "OWNER", &["DEVELOPER"]);
    }
    assert_preselection(&requests[2], "GROUP", &["SOURCE_LIBRARY"]);
    assert_output_facet(&requests[2], Some("TYPE"));
    assert_preselection(&requests[3], "GROUP", &["SOURCE_LIBRARY"]);
    assert_output_facet(&requests[3], None);
}

#[tokio::test]
async fn adaptive_refresh_can_skip_a_repeated_same_facet_level() {
    let (client, state) = client(Behavior::AdaptiveHierarchyRefresh).await;
    let vfs = VirtualRepositoryTree::builder(client)
        .mount(selection_mount("Objects").facet_policy(FacetPolicy::new([
            FacetLevel::adaptive(RepositoryFacet::APPLICATION_COMPONENT, 10),
            FacetLevel::always(RepositoryFacet::TYPE),
        ])))
        .build()
        .await
        .unwrap();
    let mount = vfs.children(vfs.root()).await.unwrap().remove(0);
    let root_component = vfs.children(mount.id).await.unwrap().remove(0);
    let old_leaf = vfs.children(root_component.id).await.unwrap().remove(0);

    let refreshed = vfs.refresh(root_component.id).await.unwrap();

    assert_eq!(refreshed[0].label, "Classes");
    assert!(vfs.node(old_leaf.id).is_none());
    assert!(matches!(
        vfs.node(root_component.id).unwrap().kind,
        NodeKind::Facet {
            object_count: 3,
            has_children_of_same_facet: true,
            ..
        }
    ));
    let requests = state.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    for request in requests.iter() {
        assert_preselection(request, "OWNER", &["DEVELOPER"]);
    }
    assert_preselection(&requests[1], "APPL", &["ROOT_APPL"]);
    assert_output_facet(&requests[1], Some("APPL"));
    assert_preselection(&requests[2], "APPL", &["ROOT_APPL"]);
    assert_output_facet(&requests[2], Some("APPL"));
    assert_preselection(&requests[3], "APPL", &["ROOT_APPL"]);
    assert!(!requests[3].contains("LEAF_APPL"));
    assert_output_facet(&requests[3], Some("TYPE"));
}

#[tokio::test]
async fn scopes_loading_locks_to_individual_nodes() {
    let (client, state) = client(Behavior::SlowEmpty).await;
    let vfs = VirtualRepositoryTree::builder(client)
        .mount(flat_selection_mount("First"))
        .mount(flat_selection_mount("Second"))
        .build()
        .await
        .unwrap();
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
    let vfs = VirtualRepositoryTree::builder(client)
        .mount(flat_selection_mount("Objects"))
        .build()
        .await
        .unwrap();
    let mount = vfs.children(vfs.root()).await.unwrap().remove(0);

    let (first, second) = tokio::join!(vfs.children(mount.id), vfs.children(mount.id));

    assert!(first.unwrap().is_empty());
    assert!(second.unwrap().is_empty());
    assert_eq!(state.post_count.load(Ordering::SeqCst), 1);
    assert_output_facet(&state.requests.lock().unwrap()[0], None);
}

#[tokio::test]
async fn ancestor_refresh_does_not_leave_orphans_from_a_descendant_load() {
    let (client, state) = client(Behavior::AncestorRefreshRace).await;
    let vfs = VirtualRepositoryTree::builder(client)
        .mount(
            selection_mount("Objects").facet_policy(FacetPolicy::grouped([RepositoryFacet::GROUP])),
        )
        .build()
        .await
        .unwrap();
    let mount = vfs.children(vfs.root()).await.unwrap().remove(0);
    let descendant = vfs.children(mount.id).await.unwrap().remove(0);

    let descendant_load = vfs.children(descendant.id);
    let ancestor_refresh = async {
        state.descendant_started.notified().await;
        let refreshed = vfs.refresh(mount.id).await;
        state.release_descendant.notify_one();
        refreshed
    };

    let (descendant_result, refreshed) = tokio::join!(descendant_load, ancestor_refresh);
    let replacement = refreshed.unwrap().remove(0);

    assert!(matches!(
        descendant_result,
        Err(VfsError::StaleNode(id)) if id == descendant.id
    ));
    assert!(
        vfs.node(successor(replacement.id)).is_none(),
        "a stale descendant load inserted an orphan node"
    );
}

#[tokio::test]
async fn retries_failed_expansions_instead_of_caching_the_error() {
    let (client, state) = client(Behavior::FailOnce).await;
    let vfs = VirtualRepositoryTree::builder(client)
        .mount(flat_selection_mount("Objects"))
        .build()
        .await
        .unwrap();
    let mount = vfs.children(vfs.root()).await.unwrap().remove(0);

    assert!(vfs.children(mount.id).await.is_err());
    assert!(vfs.children(mount.id).await.unwrap().is_empty());
    assert_eq!(state.post_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn refresh_replaces_descendants_and_invalidates_old_ids() {
    let (client, _) = client(Behavior::Refresh).await;
    let vfs = VirtualRepositoryTree::builder(client)
        .mount(flat_selection_mount("Objects"))
        .build()
        .await
        .unwrap();
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
    let vfs = VirtualRepositoryTree::builder(client)
        .mount(flat_selection_mount("Objects"))
        .build()
        .await
        .unwrap();
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
    let first = VirtualRepositoryTree::builder(first_client)
        .build()
        .await
        .unwrap();
    let second = VirtualRepositoryTree::builder(second_client)
        .build()
        .await
        .unwrap();

    assert_ne!(first.root(), second.root());
    assert!(first.node(second.root()).is_none());
    assert!(matches!(
        first.children(second.root()).await,
        Err(VfsError::UnknownNode(id)) if id == second.root()
    ));
}
