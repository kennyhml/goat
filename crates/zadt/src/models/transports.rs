use std::{borrow::Cow, fmt};

use serde::{Deserialize, Serialize};

use crate::{AdtUri, CtsError};

const ABAP_XML_NAMESPACE: &str = "http://www.sap.com/abapxml";
const LEGACY_TRANSPORT_REFERENCE_PREFIX: &str = "/com.sap.cts/object_record/";

/// The CTS function assigned to a transport request.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum TransportKind {
    /// A Workbench transport (`K`).
    Workbench,

    /// A Customizing transport (`W`).
    Customizing,

    /// Another CTS transport function retained by its wire value.
    Other(String),
}

impl TransportKind {
    /// Returns the exact CTS `TRFUNCTION` value.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Workbench => "K",
            Self::Customizing => "W",
            Self::Other(value) => value,
        }
    }

    fn parse(value: String) -> Self {
        match value.as_str() {
            "K" => Self::Workbench,
            "W" => Self::Customizing,
            _ => Self::Other(value),
        }
    }
}

impl fmt::Display for TransportKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// An open CTS transport status value.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TransportStatus(Cow<'static, str>);

impl TransportStatus {
    /// The request can be modified (`D`).
    pub const MODIFIABLE: Self = Self(Cow::Borrowed("D"));

    /// The request can be modified but is protected (`L`).
    pub const MODIFIABLE_PROTECTED: Self = Self(Cow::Borrowed("L"));

    /// Release of the request has started (`O`).
    pub const RELEASE_STARTED: Self = Self(Cow::Borrowed("O"));

    /// The request has been released (`R`).
    pub const RELEASED: Self = Self(Cow::Borrowed("R"));

    /// The request is released with import protection for repaired objects (`N`).
    pub const RELEASED_WITH_IMPORT_PROTECTION: Self = Self(Cow::Borrowed("N"));

    /// The request is being prepared for release (`P`).
    pub const RELEASE_PREPARATION: Self = Self(Cow::Borrowed("P"));

    /// Returns the exact CTS `TRSTATUS` value.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn parse(value: String) -> Self {
        Self(Cow::Owned(value))
    }
}

impl fmt::Display for TransportStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One CTS transport request header returned by ADT.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransportRequest {
    /// The transport request number (`TRKORR`).
    pub number: String,

    /// The request's CTS transport function.
    pub kind: TransportKind,

    /// The CTS transport status.
    pub status: TransportStatus,

    /// The transport target system, when assigned.
    pub target_system: Option<String>,

    /// The request owner (`AS4USER`).
    pub owner: String,

    /// The CTS date value (`AS4DATE`).
    pub date: String,

    /// The CTS time value (`AS4TIME`).
    pub time: String,

    /// The transport description (`AS4TEXT`).
    pub description: String,

    /// The SAP client, when supplied.
    pub client: Option<String>,

    /// The repository identifier, when supplied.
    pub repository_id: Option<String>,
}

/// The result of checking transport recording for one repository resource.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransportCheckResult {
    /// The CTS object key resolved from the requested ADT resource.
    pub object: TransportObjectKey,

    /// The operation echoed by the backend (`I` for insertion, empty for modification).
    pub operation: String,

    /// The package resolved for the object.
    pub package: Option<String>,

    /// Human-readable package or object information returned by CTS.
    pub object_info_text: Option<String>,

    /// The transport layer inherited by the package.
    pub transport_layer: Option<String>,

    /// The package software component or delivery unit.
    pub software_component: Option<String>,

    /// The package's super package, when present.
    pub super_package: Option<String>,

    /// Whether changes are recorded for the resolved package.
    pub package_recording_active: bool,

    /// Whether recording changes was requested explicitly.
    pub record_changes: bool,

    /// Whether the CTS check completed successfully.
    pub action_successful: bool,

    /// Whether the checked operation must be recorded in a transport request.
    pub recording_required: bool,

    /// Whether only an already-associated request may be selected.
    pub existing_request_only: bool,

    /// Informational and error messages returned by CTS.
    pub messages: Vec<TransportCheckMessage>,

    /// Transport requests offered for recording, in backend order.
    pub requests: Vec<TransportRequest>,

    /// Existing object locks relevant to the checked operation.
    pub locks: Vec<TransportObjectLock>,

    /// CTS project metadata returned for candidate requests.
    pub projects: Vec<TransportProject>,

    /// The object's package from TADIR, when supplied separately.
    pub tadir_package: Option<String>,
}

impl TransportCheckResult {
    pub(crate) fn parse(body: &[u8]) -> Result<Self, CtsError> {
        let raw: RawTransportCheckResponse =
            serde_xml_rs::from_reader(body).map_err(CtsError::InvalidTransportResponse)?;
        let data = raw.values.data;

        Ok(Self {
            object: TransportObjectKey {
                program_id: data.program_id,
                object_type: data.object_type,
                object_name: data.object_name,
            },
            operation: data.operation,
            package: non_empty(data.package),
            object_info_text: non_empty(data.object_info_text),
            transport_layer: non_empty(data.transport_layer),
            software_component: non_empty(data.software_component),
            super_package: non_empty(data.super_package),
            package_recording_active: is_abap_true(&data.package_recording_active),
            record_changes: is_abap_true(&data.record_changes),
            action_successful: data.result.eq_ignore_ascii_case("S"),
            recording_required: is_abap_true(&data.recording),
            existing_request_only: is_abap_true(&data.existing_request_only),
            messages: data
                .messages
                .messages
                .into_iter()
                .map(TransportCheckMessage::from)
                .collect(),
            requests: data
                .requests
                .requests
                .into_iter()
                .map(|request| request.header.into())
                .collect(),
            locks: data
                .locks
                .locks
                .into_iter()
                .map(TransportObjectLock::from)
                .collect(),
            projects: data
                .projects
                .projects
                .into_iter()
                .map(TransportProject::from)
                .collect(),
            tadir_package: non_empty(data.tadir_package),
        })
    }
}

/// A CTS object key resolved by a transport check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransportObjectKey {
    /// The CTS program ID, such as `R3TR` or `LIMU`.
    pub program_id: String,

    /// The CTS object type, such as `CLAS`, `CINC`, or `REPS`.
    pub object_type: String,

    /// The CTS object name.
    pub object_name: String,
}

/// A message emitted while checking transport recording.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransportCheckMessage {
    /// The backend-defined message severity.
    pub severity: String,

    /// The SAP language key (`SPRSL`).
    pub language: String,

    /// The SAP message class (`ARBGB`).
    pub message_class: String,

    /// The message number, preserving leading zeroes.
    pub message_number: String,

    /// The four message variables in backend order.
    pub variables: Vec<String>,

    /// The rendered message text.
    pub text: String,
}

/// One CTS lock relevant to a transport check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransportObjectLock {
    /// The exact CTS object key held by the request.
    pub object: TransportObjectKey,

    /// The request holding the object lock.
    pub holder: TransportRequest,

    /// Tasks beneath the lock-holding request.
    pub tasks: Vec<TransportTask>,
}

/// A CTS task returned beneath a lock-holding request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransportTask {
    /// The CTS task number.
    pub number: String,

    /// The task's CTS function.
    pub kind: TransportKind,

    /// The CTS task status.
    pub status: TransportStatus,

    /// The task owner.
    pub owner: String,

    /// The CTS date value.
    pub date: String,

    /// The CTS time value.
    pub time: String,

    /// The task description.
    pub description: String,
}

/// CTS project metadata associated with a candidate request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransportProject {
    /// The request associated with this project entry.
    pub request_number: String,

    /// The external CTS project identifier.
    pub id: String,

    /// The CTS project description.
    pub description: String,
}

/// The result of creating a CTS transport request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransportCreation {
    /// The newly created transport request number.
    pub transport_number: String,

    /// An optional status message returned by integrated change management.
    pub message: Option<TransportCreationMessage>,
}

impl TransportCreation {
    pub(crate) fn parse(body: &[u8]) -> Result<Self, CtsError> {
        let raw: RawTransportCreation =
            serde_xml_rs::from_reader(body).map_err(CtsError::InvalidTransportResponse)?;
        if raw.values.data.transport_number.is_empty() {
            return Err(CtsError::MissingTransportCreationResponse);
        }

        let message = raw.values.data.message;
        Ok(Self {
            transport_number: raw.values.data.transport_number,
            message: (!message.severity.is_empty()
                || !message.short_text.is_empty()
                || !message.long_text.is_empty())
            .then_some(TransportCreationMessage {
                severity: message.severity,
                short_text: message.short_text,
                long_text: message.long_text,
            }),
        })
    }

    pub(crate) fn parse_legacy(body: &[u8]) -> Result<Self, CtsError> {
        let reference = std::str::from_utf8(body)
            .map_err(CtsError::InvalidTransportCreationResponseEncoding)?
            .trim();
        let Some(transport_number) = reference
            .strip_prefix(LEGACY_TRANSPORT_REFERENCE_PREFIX)
            .filter(|number| !number.is_empty() && !number.contains('/'))
        else {
            return Err(CtsError::InvalidTransportCreationReference {
                reference: reference.to_owned(),
            });
        };

        Ok(Self {
            transport_number: transport_number.to_owned(),
            message: None,
        })
    }
}

/// A status message attached to a created transport request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransportCreationMessage {
    /// The backend-defined message severity.
    pub severity: String,

    /// The localized short message text.
    pub short_text: String,

    /// Optional HTML long text.
    pub long_text: String,
}

impl TransportRequest {
    pub(crate) fn parse(body: &[u8]) -> Result<Self, CtsError> {
        let raw: RawTransportRequestResponse =
            serde_xml_rs::from_reader(body).map_err(CtsError::InvalidTransportResponse)?;
        Ok(raw.values.data.into())
    }
}

/// Transport requests returned by a [`crate::TransportsQuery`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TransportRequests {
    /// The returned request headers in backend order.
    pub requests: Vec<TransportRequest>,
}

impl TransportRequests {
    pub(crate) fn parse(body: &[u8]) -> Result<Self, CtsError> {
        if body.is_empty() {
            return Ok(Self::default());
        }

        let raw: RawTransportRequests =
            serde_xml_rs::from_reader(body).map_err(CtsError::InvalidTransportResponse)?;
        Ok(Self {
            requests: raw
                .values
                .data
                .requests
                .into_iter()
                .map(TransportRequest::from)
                .collect(),
        })
    }

    /// Returns the number of transport requests.
    pub fn len(&self) -> usize {
        self.requests.len()
    }

    /// Returns whether no transport requests were found.
    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }
}

impl From<RawTransportRequest> for TransportRequest {
    fn from(raw: RawTransportRequest) -> Self {
        Self {
            number: raw.number,
            kind: TransportKind::parse(raw.kind),
            status: TransportStatus::parse(raw.status),
            target_system: non_empty(raw.target_system),
            owner: raw.owner,
            date: raw.date,
            time: raw.time,
            description: raw.description,
            client: non_empty(raw.client),
            repository_id: non_empty(raw.repository_id),
        }
    }
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn is_abap_true(value: &str) -> bool {
    value.eq_ignore_ascii_case("X")
}

#[derive(Serialize)]
#[serde(rename = "asx:abap")]
pub(crate) struct TransportCheckRequest<'a> {
    #[serde(rename = "@version")]
    version: &'static str,

    #[serde(rename = "asx:values")]
    values: RawTransportCheckRequestValues<'a>,
}

impl<'a> TransportCheckRequest<'a> {
    pub(crate) fn new(
        uri: &'a AdtUri,
        operation: &'static str,
        package: Option<&'a str>,
        super_package: Option<&'a str>,
        record_changes: bool,
    ) -> Self {
        Self {
            version: "1.0",
            values: RawTransportCheckRequestValues {
                data: RawTransportCheckRequestData {
                    program_id: "",
                    object_type: "",
                    object_name: "",
                    package: package.unwrap_or_default(),
                    super_package: super_package.unwrap_or_default(),
                    record_changes: if record_changes { "X" } else { "" },
                    operation,
                    uri: uri.as_str(),
                },
            },
        }
    }

    pub(crate) fn serialize(&self) -> Result<String, CtsError> {
        serde_xml_rs::SerdeXml::new()
            .namespace("asx", ABAP_XML_NAMESPACE)
            .to_string(self)
            .map_err(CtsError::InvalidTransportCheckRequest)
    }
}

#[derive(Serialize)]
struct RawTransportCheckRequestValues<'a> {
    #[serde(rename = "DATA")]
    data: RawTransportCheckRequestData<'a>,
}

#[derive(Serialize)]
struct RawTransportCheckRequestData<'a> {
    #[serde(rename = "PGMID")]
    program_id: &'static str,

    #[serde(rename = "OBJECT")]
    object_type: &'static str,

    #[serde(rename = "OBJECTNAME")]
    object_name: &'static str,

    #[serde(rename = "DEVCLASS")]
    package: &'a str,

    #[serde(rename = "SUPER_PACKAGE")]
    super_package: &'a str,

    #[serde(rename = "RECORD_CHANGES")]
    record_changes: &'static str,

    #[serde(rename = "OPERATION")]
    operation: &'static str,

    #[serde(rename = "URI")]
    uri: &'a str,
}

#[derive(Serialize)]
#[serde(rename = "asx:abap")]
pub(crate) struct TransportCreateRequest<'a> {
    #[serde(rename = "@version")]
    version: &'static str,

    #[serde(rename = "asx:values")]
    values: RawTransportCreateValues<'a>,
}

impl<'a> TransportCreateRequest<'a> {
    pub(crate) fn new(
        package: Option<&'a str>,
        description: &'a str,
        reference: Option<&'a AdtUri>,
    ) -> Self {
        Self {
            version: "1.0",
            values: RawTransportCreateValues {
                data: RawTransportCreateData {
                    operation: "I",
                    package: package.unwrap_or_default(),
                    description,
                    reference: reference.map(AdtUri::as_str),
                },
            },
        }
    }

    pub(crate) fn serialize(&self) -> Result<String, CtsError> {
        serde_xml_rs::SerdeXml::new()
            .namespace("asx", ABAP_XML_NAMESPACE)
            .to_string(self)
            .map_err(CtsError::InvalidTransportCreationRequest)
    }
}

#[derive(Serialize)]
struct RawTransportCreateValues<'a> {
    #[serde(rename = "DATA")]
    data: RawTransportCreateData<'a>,
}

#[derive(Serialize)]
struct RawTransportCreateData<'a> {
    #[serde(rename = "OPERATION")]
    operation: &'static str,

    #[serde(rename = "DEVCLASS")]
    package: &'a str,

    #[serde(rename = "REQUEST_TEXT")]
    description: &'a str,

    #[serde(rename = "REF", skip_serializing_if = "Option::is_none")]
    reference: Option<&'a str>,
}

#[derive(Deserialize)]
#[serde(rename = "asx:abap")]
struct RawTransportCheckResponse {
    #[serde(rename = "asx:values")]
    values: RawTransportCheckValues,
}

#[derive(Deserialize)]
struct RawTransportCheckValues {
    #[serde(rename = "DATA")]
    data: RawTransportCheckData,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RawTransportCheckData {
    #[serde(rename = "PGMID")]
    program_id: String,

    #[serde(rename = "OBJECT")]
    object_type: String,

    #[serde(rename = "OBJECTNAME")]
    object_name: String,

    #[serde(rename = "OPERATION")]
    operation: String,

    #[serde(rename = "DEVCLASS")]
    package: String,

    #[serde(rename = "CTEXT")]
    object_info_text: String,

    #[serde(rename = "KORRFLAG")]
    package_recording_active: String,

    #[serde(rename = "PDEVCLASS")]
    transport_layer: String,

    #[serde(rename = "DLVUNIT")]
    software_component: String,

    #[serde(rename = "SUPER_PACKAGE")]
    super_package: String,

    #[serde(rename = "RECORD_CHANGES")]
    record_changes: String,

    #[serde(rename = "RESULT")]
    result: String,

    #[serde(rename = "RECORDING")]
    recording: String,

    #[serde(rename = "EXISTING_REQ_ONLY")]
    existing_request_only: String,

    #[serde(rename = "MESSAGES")]
    messages: RawTransportCheckMessages,

    #[serde(rename = "REQUESTS")]
    requests: RawTransportCheckRequests,

    #[serde(rename = "LOCKS")]
    locks: RawTransportCheckLocks,

    #[serde(rename = "CTS_PROJECTS")]
    projects: RawTransportProjects,

    #[serde(rename = "TADIRDEVC")]
    tadir_package: String,
}

#[derive(Default, Deserialize)]
struct RawTransportCheckMessages {
    #[serde(rename = "CTS_MESSAGE", default)]
    messages: Vec<RawTransportCheckMessage>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RawTransportCheckMessage {
    #[serde(rename = "SEVERITY")]
    severity: String,

    #[serde(rename = "SPRSL")]
    language: String,

    #[serde(rename = "ARBGB")]
    message_class: String,

    #[serde(rename = "MSGNR")]
    message_number: String,

    #[serde(rename = "VARIABLES")]
    variables: RawTransportCheckVariables,

    #[serde(rename = "TEXT")]
    text: String,
}

#[derive(Default, Deserialize)]
struct RawTransportCheckVariables {
    #[serde(rename = "CTS_VARIABLE", default)]
    variables: Vec<RawTransportCheckVariable>,
}

#[derive(Default, Deserialize)]
struct RawTransportCheckVariable {
    #[serde(rename = "VARIABLE", default)]
    value: String,
}

impl From<RawTransportCheckMessage> for TransportCheckMessage {
    fn from(raw: RawTransportCheckMessage) -> Self {
        Self {
            severity: raw.severity,
            language: raw.language,
            message_class: raw.message_class,
            message_number: raw.message_number,
            variables: raw
                .variables
                .variables
                .into_iter()
                .map(|variable| variable.value)
                .collect(),
            text: raw.text,
        }
    }
}

#[derive(Default, Deserialize)]
struct RawTransportCheckRequests {
    #[serde(rename = "CTS_REQUEST", default)]
    requests: Vec<RawTransportCheckRequestEntry>,
}

#[derive(Deserialize)]
struct RawTransportCheckRequestEntry {
    #[serde(rename = "REQ_HEADER")]
    header: RawTransportRequest,
}

#[derive(Default, Deserialize)]
struct RawTransportCheckLocks {
    #[serde(rename = "CTS_OBJECT_LOCK", default)]
    locks: Vec<RawTransportObjectLock>,
}

#[derive(Deserialize)]
struct RawTransportObjectLock {
    #[serde(rename = "OBJECT_KEY")]
    object: RawTransportObjectKey,

    #[serde(rename = "LOCK_HOLDER")]
    holder: RawTransportLockHolder,
}

#[derive(Deserialize)]
struct RawTransportObjectKey {
    #[serde(rename = "PGMID")]
    program_id: String,

    #[serde(rename = "OBJECT")]
    object_type: String,

    #[serde(rename = "OBJ_NAME")]
    object_name: String,
}

#[derive(Deserialize)]
struct RawTransportLockHolder {
    #[serde(rename = "REQ_HEADER")]
    request: RawTransportRequest,

    #[serde(rename = "TASK_HEADERS", default)]
    tasks: RawTransportTasks,
}

#[derive(Default, Deserialize)]
struct RawTransportTasks {
    #[serde(rename = "CTS_TASK_HEADER", default)]
    tasks: Vec<RawTransportTask>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RawTransportTask {
    #[serde(rename = "TRKORR")]
    number: String,

    #[serde(rename = "TRFUNCTION")]
    kind: String,

    #[serde(rename = "TRSTATUS")]
    status: String,

    #[serde(rename = "AS4USER")]
    owner: String,

    #[serde(rename = "AS4DATE")]
    date: String,

    #[serde(rename = "AS4TIME")]
    time: String,

    #[serde(rename = "AS4TEXT")]
    description: String,
}

impl From<RawTransportObjectLock> for TransportObjectLock {
    fn from(raw: RawTransportObjectLock) -> Self {
        Self {
            object: TransportObjectKey {
                program_id: raw.object.program_id,
                object_type: raw.object.object_type,
                object_name: raw.object.object_name,
            },
            holder: raw.holder.request.into(),
            tasks: raw
                .holder
                .tasks
                .tasks
                .into_iter()
                .map(TransportTask::from)
                .collect(),
        }
    }
}

impl From<RawTransportTask> for TransportTask {
    fn from(raw: RawTransportTask) -> Self {
        Self {
            number: raw.number,
            kind: TransportKind::parse(raw.kind),
            status: TransportStatus::parse(raw.status),
            owner: raw.owner,
            date: raw.date,
            time: raw.time,
            description: raw.description,
        }
    }
}

#[derive(Default, Deserialize)]
struct RawTransportProjects {
    #[serde(rename = "SADT_CTS_PROJECT", default)]
    projects: Vec<RawTransportProject>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RawTransportProject {
    #[serde(rename = "TRKORR")]
    request_number: String,

    #[serde(rename = "EXTERNALID")]
    id: String,

    #[serde(rename = "DESCRIPTN")]
    description: String,
}

impl From<RawTransportProject> for TransportProject {
    fn from(raw: RawTransportProject) -> Self {
        Self {
            request_number: raw.request_number,
            id: raw.id,
            description: raw.description,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename = "asx:abap")]
struct RawTransportRequests {
    #[serde(rename = "asx:values")]
    values: RawTransportValues,
}

#[derive(Deserialize)]
#[serde(rename = "asx:abap")]
struct RawTransportRequestResponse {
    #[serde(rename = "asx:values")]
    values: RawTransportRequestValue,
}

#[derive(Deserialize)]
struct RawTransportRequestValue {
    #[serde(rename = "DATA")]
    data: RawTransportRequest,
}

#[derive(Deserialize)]
struct RawTransportValues {
    #[serde(rename = "DATA")]
    data: RawTransportData,
}

#[derive(Deserialize)]
struct RawTransportData {
    #[serde(rename = "CTS_REQ_HEADER", default)]
    requests: Vec<RawTransportRequest>,
}

#[derive(Deserialize)]
struct RawTransportRequest {
    #[serde(rename = "TRKORR")]
    number: String,

    #[serde(rename = "TRFUNCTION")]
    kind: String,

    #[serde(rename = "TRSTATUS")]
    status: String,

    #[serde(rename = "TARSYSTEM")]
    target_system: String,

    #[serde(rename = "AS4USER")]
    owner: String,

    #[serde(rename = "AS4DATE")]
    date: String,

    #[serde(rename = "AS4TIME")]
    time: String,

    #[serde(rename = "AS4TEXT")]
    description: String,

    #[serde(rename = "CLIENT")]
    client: String,

    #[serde(rename = "REPOID")]
    repository_id: String,
}

#[derive(Deserialize)]
#[serde(rename = "asx:abap")]
struct RawTransportCreation {
    #[serde(rename = "asx:values")]
    values: RawTransportCreationValues,
}

#[derive(Deserialize)]
struct RawTransportCreationValues {
    #[serde(rename = "DATA")]
    data: RawTransportCreationData,
}

#[derive(Deserialize)]
struct RawTransportCreationData {
    #[serde(rename = "TRKORR")]
    transport_number: String,

    #[serde(rename = "MESSAGE", default)]
    message: RawTransportCreationMessage,
}

#[derive(Default, Deserialize)]
struct RawTransportCreationMessage {
    #[serde(rename = "SEVERITY", default)]
    severity: String,

    #[serde(rename = "SHORT_TEXT", default)]
    short_text: String,

    #[serde(rename = "LONG_TEXT", default)]
    long_text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRANSPORTS_XML: &[u8] = include_bytes!("../../tests/fixtures/transport-requests.xml");
    const TRANSPORT_XML: &[u8] = include_bytes!("../../tests/fixtures/transport-request.xml");
    const TRANSPORT_CHECK_XML: &[u8] = include_bytes!("../../tests/fixtures/transport-check.xml");

    #[test]
    fn parses_transport_request_headers_and_preserves_unknown_functions() {
        let transports = TransportRequests::parse(TRANSPORTS_XML).unwrap();

        assert_eq!(transports.len(), 2);
        assert_eq!(transports.requests[0].kind, TransportKind::Workbench);
        assert_eq!(transports.requests[0].target_system, None);
        assert_eq!(
            transports.requests[1].kind,
            TransportKind::Other("T".to_owned())
        );
        assert_eq!(
            transports.requests[1].repository_id.as_deref(),
            Some("ABAP")
        );
    }

    #[test]
    fn treats_an_empty_transport_response_as_an_empty_list() {
        assert!(TransportRequests::parse(&[]).unwrap().is_empty());
    }

    #[test]
    fn parses_a_single_transport_request() {
        let transport = TransportRequest::parse(TRANSPORT_XML).unwrap();

        assert_eq!(transport.number, "DEVK900001");
        assert_eq!(transport.kind, TransportKind::Workbench);
        assert_eq!(transport.client, None);
        assert_eq!(transport.description, "Workbench request");
    }

    #[test]
    fn maps_standard_transport_statuses_and_preserves_custom_values() {
        for (value, expected) in [
            ("D", TransportStatus::MODIFIABLE),
            ("L", TransportStatus::MODIFIABLE_PROTECTED),
            ("O", TransportStatus::RELEASE_STARTED),
            ("R", TransportStatus::RELEASED),
            ("N", TransportStatus::RELEASED_WITH_IMPORT_PROTECTION),
            ("P", TransportStatus::RELEASE_PREPARATION),
        ] {
            assert_eq!(TransportStatus::parse(value.to_owned()), expected);
        }
        assert_eq!(TransportStatus::parse("Z".to_owned()).as_str(), "Z");
    }

    #[test]
    fn serializes_insert_transport_check_context_as_asx() {
        let uri = AdtUri::parse("/sap/bc/adt/oo/classes/zcl_example").unwrap();
        let xml = TransportCheckRequest::new(&uri, "I", Some("ZPACKAGE"), Some("ZROOT"), true)
            .serialize()
            .unwrap();

        assert!(xml.contains("<PGMID />") || xml.contains("<PGMID></PGMID>"));
        assert!(xml.contains("<OBJECT />") || xml.contains("<OBJECT></OBJECT>"));
        assert!(xml.contains("<OBJECTNAME />") || xml.contains("<OBJECTNAME></OBJECTNAME>"));
        assert!(xml.contains("<DEVCLASS>ZPACKAGE</DEVCLASS>"));
        assert!(xml.contains("<SUPER_PACKAGE>ZROOT</SUPER_PACKAGE>"));
        assert!(xml.contains("<RECORD_CHANGES>X</RECORD_CHANGES>"));
        assert!(xml.contains("<OPERATION>I</OPERATION>"));
        assert!(xml.contains("<URI>/sap/bc/adt/oo/classes/zcl_example</URI>"));
    }

    #[test]
    fn serializes_modification_as_an_empty_operation() {
        let uri = AdtUri::parse("/sap/bc/adt/oo/classes/zcl_example").unwrap();
        let xml = TransportCheckRequest::new(&uri, "", None, None, false)
            .serialize()
            .unwrap();

        assert!(xml.contains("<OPERATION />") || xml.contains("<OPERATION></OPERATION>"));
        assert!(
            xml.contains("<RECORD_CHANGES />") || xml.contains("<RECORD_CHANGES></RECORD_CHANGES>")
        );
    }

    #[test]
    fn parses_transport_check_requests_messages_locks_and_projects() {
        let check = TransportCheckResult::parse(TRANSPORT_CHECK_XML).unwrap();

        assert_eq!(check.object.program_id, "LIMU");
        assert_eq!(check.object.object_type, "CINC");
        assert_eq!(check.operation, "I");
        assert_eq!(check.package.as_deref(), Some("ZPACKAGE"));
        assert_eq!(check.transport_layer.as_deref(), Some("ZDEV"));
        assert_eq!(check.software_component.as_deref(), Some("HOME"));
        assert!(check.package_recording_active);
        assert!(check.record_changes);
        assert!(check.action_successful);
        assert!(check.recording_required);
        assert!(check.existing_request_only);

        assert_eq!(check.messages.len(), 1);
        assert_eq!(check.messages[0].message_number, "007");
        assert_eq!(
            check.messages[0].variables,
            ["Z", "R3TR", "CLAS", "ZCL_EXAMPLE"]
        );

        assert_eq!(check.requests.len(), 1);
        assert_eq!(check.requests[0].number, "DEVK900001");
        assert_eq!(check.requests[0].target_system.as_deref(), Some("QAS"));

        assert_eq!(check.locks.len(), 1);
        assert_eq!(check.locks[0].holder.number, "DEVK900001");
        assert_eq!(check.locks[0].tasks.len(), 1);
        assert_eq!(check.locks[0].tasks[0].number, "DEVK900002");
        assert_eq!(
            check.locks[0].tasks[0].kind,
            TransportKind::Other("R".to_owned())
        );

        assert_eq!(check.projects.len(), 1);
        assert_eq!(check.projects[0].id, "PROJECT-1");
        assert_eq!(check.tadir_package.as_deref(), Some("ZPACKAGE"));
    }

    #[test]
    fn accepts_empty_transport_check_collections() {
        let xml = br#"<asx:abap xmlns:asx="http://www.sap.com/abapxml" version="1.0">
            <asx:values><DATA><RESULT>E</RESULT><MESSAGES/><REQUESTS/><LOCKS/>
            <CTS_PROJECTS/></DATA></asx:values></asx:abap>"#;

        let check = TransportCheckResult::parse(xml).unwrap();

        assert!(!check.action_successful);
        assert!(!check.recording_required);
        assert!(check.messages.is_empty());
        assert!(check.requests.is_empty());
        assert!(check.locks.is_empty());
        assert!(check.projects.is_empty());
    }

    #[test]
    fn serializes_transport_creation_as_asx() {
        let reference = AdtUri::parse("/sap/bc/adt/packages/zpackage").unwrap();
        let xml =
            TransportCreateRequest::new(Some("ZPACKAGE"), "Create <transport>", Some(&reference))
                .serialize()
                .unwrap();

        assert!(xml.contains("<OPERATION>I</OPERATION>"));
        assert!(xml.contains("<DEVCLASS>ZPACKAGE</DEVCLASS>"));
        assert!(xml.contains("<REQUEST_TEXT>Create &lt;transport&gt;</REQUEST_TEXT>"));
        assert!(xml.contains("<REF>/sap/bc/adt/packages/zpackage</REF>"));
    }

    #[test]
    fn omits_an_unset_transport_reference() {
        let xml = TransportCreateRequest::new(None, "Create transport", None)
            .serialize()
            .unwrap();

        assert!(xml.contains("<DEVCLASS />") || xml.contains("<DEVCLASS></DEVCLASS>"));
        assert!(!xml.contains("<REF"));
    }

    #[test]
    fn parses_modern_and_legacy_transport_creation_responses() {
        let modern = br#"<asx:abap xmlns:asx="http://www.sap.com/abapxml" version="1.0">
            <asx:values><DATA><TRKORR>DEVK900003</TRKORR><MESSAGE>
            <SEVERITY>WARNING</SEVERITY><SHORT_TEXT>Assigned with warning</SHORT_TEXT>
            <LONG_TEXT></LONG_TEXT></MESSAGE></DATA></asx:values></asx:abap>"#;

        let modern = TransportCreation::parse(modern).unwrap();
        assert_eq!(modern.transport_number, "DEVK900003");
        assert_eq!(modern.message.unwrap().severity, "WARNING");

        let legacy =
            TransportCreation::parse_legacy(b"/com.sap.cts/object_record/DEVK900004\n").unwrap();
        assert_eq!(legacy.transport_number, "DEVK900004");
        assert_eq!(legacy.message, None);
    }
}
