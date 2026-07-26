use async_trait::async_trait;

use crate::{AdtRequest, AdtResponse, TransportError};

#[cfg(feature = "reqwest")]
mod reqwest_transport;

#[cfg(feature = "reqwest")]
pub use reqwest_transport::{ReqwestTransport, ReqwestTransportBuilder};

/// Carries transport agnostics ADT requests to an SAP system.
///
/// Implementations may use HTTP, RFC, or another mechanism while preserving
/// the operations HTTP-like ADT method, target, headers, query, and body.
#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, request: AdtRequest) -> Result<AdtResponse, TransportError>;
}
