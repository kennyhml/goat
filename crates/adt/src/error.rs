use std::{error::Error as StdError, fmt};

use http::StatusCode;
use thiserror::Error;

use crate::AdtUriError;

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

/// An error resolving a program resource from central discovery.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ProgramError {
    #[error("program name `{name}` is empty or contains invalid whitespace or control characters")]
    InvalidName { name: String },

    #[error("central discovery did not advertise the programs collection")]
    MissingCollection,

    #[error("could not construct the program resource URI: {0}")]
    InvalidTarget(#[from] AdtUriError),
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

    #[error(transparent)]
    Discovery(#[from] DiscoveryError),

    #[error(transparent)]
    Object(#[from] ObjectError),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OperationError {
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
