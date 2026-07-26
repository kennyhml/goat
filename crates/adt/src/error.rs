use std::{error::Error as StdError, fmt};

use http::StatusCode;
use thiserror::Error;

use crate::{AdtUriError, CategoryId, CompatibilityError};

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

/// An error resolving or decoding a program resource.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ProgramError {
    #[error(transparent)]
    Compatibility(#[from] CompatibilityError),

    #[error("program name `{name}` is empty or contains invalid whitespace or control characters")]
    InvalidName { name: String },

    #[error("the program-run collection did not advertise its execution template")]
    MissingRunTemplate,

    #[error("the program-run template does not support profiling")]
    UnsupportedProfiler,

    #[error("invalid program-run template `{template}`: {reason}")]
    InvalidRunTemplate { template: String, reason: String },

    #[error("program-run template expanded to invalid target `{target}`: {source}")]
    InvalidRunTarget { target: String, source: AdtUriError },

    #[error("program-run response was not valid UTF-8: {0}")]
    InvalidRunOutputEncoding(#[source] std::string::FromUtf8Error),

    #[error("could not construct the program resource URI: {0}")]
    InvalidTarget(#[from] AdtUriError),

    #[error("invalid program XML: {0}")]
    InvalidResponse(#[source] serde_xml_rs::Error),

    #[error("program link `{href}` could not be resolved: {source}")]
    InvalidLink { href: String, source: AdtUriError },

    #[error("program package URI `{uri}` is invalid: {source}")]
    InvalidPackageUri { uri: String, source: AdtUriError },

    #[error("program response did not advertise a plain-text source link")]
    MissingSourceLink,

    #[error("unsupported program object version `{version}`")]
    UnsupportedObjectVersion { version: String },

    #[error("program source attribute `{declared}` disagrees with source relation `{advertised}`")]
    SourceLinkMismatch {
        declared: String,
        advertised: String,
    },
}

/// An error resolving or decoding an ABAP include resource.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum IncludeError {
    #[error(transparent)]
    Compatibility(#[from] CompatibilityError),

    #[error("include name `{name}` is empty or contains invalid whitespace or control characters")]
    InvalidName { name: String },

    #[error("could not construct the include resource URI: {0}")]
    InvalidTarget(#[from] AdtUriError),

    #[error("invalid include XML: {0}")]
    InvalidResponse(#[source] serde_xml_rs::Error),

    #[error("include link `{href}` could not be resolved: {source}")]
    InvalidLink { href: String, source: AdtUriError },

    #[error("include package URI `{uri}` is invalid: {source}")]
    InvalidPackageUri { uri: String, source: AdtUriError },

    #[error("include context URI `{uri}` is invalid: {source}")]
    InvalidContextUri { uri: String, source: AdtUriError },

    #[error("include response did not advertise a plain-text source link")]
    MissingSourceLink,

    #[error("unsupported include object version `{version}`")]
    UnsupportedObjectVersion { version: String },

    #[error("include source attribute `{declared}` disagrees with source relation `{advertised}`")]
    SourceLinkMismatch {
        declared: String,
        advertised: String,
    },
}

/// An error in a generic ADT object operation or representation.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ObjectError {
    #[error("invalid object lock response: {0}")]
    InvalidLockResponse(#[from] serde_xml_rs::Error),

    #[error("object lock response did not contain a lock handle")]
    MissingLockHandle,

    #[error("source response was not valid UTF-8: {0}")]
    InvalidSourceEncoding(#[from] std::string::FromUtf8Error),

    #[error("lock for `{actual}` cannot be used with object `{expected}`")]
    LockHandleObjectMismatch { expected: String, actual: String },
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
    Program(#[from] ProgramError),

    #[error(transparent)]
    Include(#[from] IncludeError),
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
