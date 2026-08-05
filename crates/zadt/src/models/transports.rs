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
