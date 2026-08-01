use std::{error::Error as StdError, fmt};

use http::StatusCode;
use thiserror::Error;

use crate::{
    AdtUriError, CategoryId, CompatibilityError, GlobalWorkbenchType, resource::AdtLinkError,
};

#[cfg(feature = "reqwest")]
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ReqwestTransportBuildError {
    #[error("required client field `{0}` was not provided")]
    MissingField(&'static str),

    #[error("invalid SAP destination: {0}")]
    InvalidDestination(#[from] url::ParseError),

    #[error("SAP destination must use HTTP or HTTPS")]
    UnsupportedScheme,

    #[error("SAP destination must not contain credentials, a query, or a fragment")]
    InvalidDestinationComponents,

    #[cfg(feature = "reqwest")]
    #[error("could not construct the HTTP client: {0}")]
    HttpClient(#[from] reqwest::Error),
}

#[cfg(feature = "reqwest")]
impl From<derive_builder::UninitializedFieldError> for ReqwestTransportBuildError {
    fn from(error: derive_builder::UninitializedFieldError) -> Self {
        Self::MissingField(error.field_name())
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DiscoveryError {
    #[error("invalid discovery XML: {0}")]
    Xml(#[from] serde_xml_rs::Error),

    #[error("discovery collection `{title}` has no href")]
    MissingCollectionHref { title: String },

    #[error("discovery collection `{title}` has an invalid href `{href}`: {source}")]
    InvalidCollectionHref {
        title: String,
        href: String,
        source: AdtUriError,
    },
}

/// An error decoding or validating the HTTP session established during logon.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LogonError {
    #[error("invalid HTTP session-information XML: {0}")]
    InvalidResponse(#[from] serde_xml_rs::Error),

    #[error("HTTP session response did not include a Content-Type header")]
    MissingContentType,

    #[error("HTTP session response did not include a representation body")]
    MissingResponseBody,

    #[error("unsupported HTTP session response Content-Type `{content_type}`")]
    UnsupportedContentType { content_type: String },

    #[error("HTTP session response did not advertise a logoff resource")]
    MissingLogoffLink,

    #[error("HTTP session response did not advertise a cleanup resource")]
    MissingCleanupLink,

    #[error("system-information link did not advertise a Content-Type")]
    MissingSystemInformationContentType,

    #[error("invalid HTTP session link `{href}` for relation `{relation}`")]
    InvalidLink { relation: String, href: String },

    #[error("HTTP session response advertised inactivityTimeout more than once")]
    DuplicateInactivityTimeout,

    #[error("invalid HTTP session inactivity timeout `{value}`: {source}")]
    InvalidInactivityTimeout {
        value: String,
        source: std::num::ParseIntError,
    },
}

/// An error in a generic ADT object operation or representation.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ObjectError {
    #[error(transparent)]
    Compatibility(#[from] CompatibilityError),

    #[error("object name `{name}` is empty or contains invalid whitespace or control characters")]
    InvalidName { name: String },

    #[error("object name `{name}` exceeds the maximum length of {maximum_length} characters")]
    NameTooLong { name: String, maximum_length: usize },

    #[error("could not construct the object resource URI: {0}")]
    InvalidTarget(#[from] AdtUriError),

    #[error("invalid object representation: {0}")]
    InvalidResponse(#[source] serde_xml_rs::Error),

    #[error("object link `{href}` could not be resolved: {source}")]
    InvalidLink { href: String, source: AdtUriError },

    #[error("object response did not advertise the required `{relation}` relation")]
    MissingRelation { relation: &'static str },

    #[error("unsupported object version `{version}`")]
    UnsupportedObjectVersion { version: String },

    #[error(
        "object `{relation}` reference `{declared}` disagrees with advertised relation `{advertised}`"
    )]
    RelationMismatch {
        relation: &'static str,
        declared: String,
        advertised: String,
    },

    #[error("expected object type `{expected}`, but the response advertised `{actual}`")]
    UnexpectedObjectType {
        expected: GlobalWorkbenchType,
        actual: GlobalWorkbenchType,
    },

    #[error("expected compact object type `{expected}`, but the response advertised `{actual}`")]
    UnexpectedCompactObjectType {
        expected: &'static str,
        actual: String,
    },

    #[error("object reference is missing required field `{field}`")]
    IncompleteObjectReference { field: &'static str },

    #[error("the `{relation}` operation template was not advertised")]
    MissingTemplate { relation: &'static str },

    #[error("the operation template does not support `{parameter}`")]
    UnsupportedTemplateParameter { parameter: &'static str },

    #[error("invalid operation template `{template}`: {reason}")]
    InvalidTemplate { template: String, reason: String },

    #[error("operation template expanded to invalid target `{target}`: {source}")]
    InvalidExpandedTarget { target: String, source: AdtUriError },

    #[error("invalid object lock response: {0}")]
    InvalidLockResponse(#[from] serde_xml_rs::Error),

    #[error("object lock response did not contain a lock handle")]
    MissingLockHandle,

    #[error("object response was not valid UTF-8: {0}")]
    InvalidResponseEncoding(#[from] std::string::FromUtf8Error),

    #[error("lock for `{actual}` cannot be used with object `{expected}`")]
    LockHandleObjectMismatch { expected: String, actual: String },
}

/// An error encoding or decoding repository information system data.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RepositoryError {
    #[error("could not serialize repository information request: {0}")]
    InvalidRequest(#[source] serde_xml_rs::Error),

    #[error("invalid repository information response: {0}")]
    InvalidResponse(#[source] serde_xml_rs::Error),

    #[error("repository object `{name}` advertised invalid URI `{uri}`: {source}")]
    InvalidObjectUri {
        name: String,
        uri: String,
        source: AdtUriError,
    },
}

impl From<AdtLinkError> for ObjectError {
    fn from(error: AdtLinkError) -> Self {
        let (href, source) = error.into_parts();
        Self::InvalidLink { href, source }
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ResponseError {
    #[error("ADT returned unexpected HTTP status {status}: {body}")]
    UnexpectedStatus { status: StatusCode, body: String },

    #[error("ADT returned 304 Not Modified without an If-None-Match validator")]
    UnexpectedNotModified,

    #[error("ADT response for collection {category:?} did not include a Content-Type header")]
    MissingContentType { category: CategoryId },

    #[error(
        "ADT response for collection {category:?} used unsupported Content-Type `{content_type}`; supported media types: {supported:?}"
    )]
    UnsupportedContentType {
        category: CategoryId,
        content_type: String,
        supported: Vec<String>,
    },

    #[error(transparent)]
    Discovery(#[from] DiscoveryError),

    #[error(transparent)]
    Logon(#[from] LogonError),

    #[error(transparent)]
    Object(#[from] ObjectError),

    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OperationError {
    #[error(transparent)]
    Compatibility(#[from] CompatibilityError),

    #[error(transparent)]
    Transport(#[from] TransportError),

    #[error(transparent)]
    Response(#[from] ResponseError),

    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

/// An error produced while carrying a request through a transport.
#[derive(Debug)]
pub struct TransportError {
    source: Box<dyn StdError + Send + Sync>,
}

impl TransportError {
    pub fn new(error: impl StdError + Send + Sync + 'static) -> Self {
        Self {
            source: Box::new(error),
        }
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl StdError for TransportError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.source.as_ref())
    }
}
