use std::fmt;

use serde::Deserialize;

use crate::CtsError;

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

/// An open CTS transport status value such as `D` or `R`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TransportStatus(String);

impl TransportStatus {
    /// Returns the exact CTS `TRSTATUS` value.
    pub fn as_str(&self) -> &str {
        &self.0
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

    /// The raw CTS transport status.
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
            status: TransportStatus(raw.status),
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

#[derive(Deserialize)]
#[serde(rename = "asx:abap")]
struct RawTransportRequests {
    #[serde(rename = "asx:values")]
    values: RawTransportValues,
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

#[cfg(test)]
mod tests {
    use super::*;

    const TRANSPORTS_XML: &[u8] = include_bytes!("../../tests/fixtures/transport-requests.xml");

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
}
