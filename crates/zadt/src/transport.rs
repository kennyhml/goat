use async_trait::async_trait;

use crate::{AdtRequest, AdtResponse, TransportError};

#[cfg(feature = "reqwest")]
mod reqwest_transport;
#[cfg(feature = "logging")]
mod traced;

#[cfg(feature = "reqwest")]
pub use reqwest_transport::{ReqwestTransport, ReqwestTransportBuilder};
#[cfg(feature = "logging")]
pub use traced::Traced;

/// Carries transport agnostics ADT requests to an SAP system.
///
/// Implementations may use HTTP, RFC, or another mechanism while preserving
/// the operations HTTP-like ADT method, target, headers, query, and body.
#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, request: AdtRequest) -> Result<AdtResponse, TransportError>;
}

/// Convenience decorators available for every concrete [`Transport`].
#[cfg(feature = "logging")]
pub trait TransportExt: Transport + Sized {
    /// Wraps this transport with structured ADT call tracing.
    fn traced(self) -> Traced<Self> {
        Traced::new(self)
    }
}

#[cfg(feature = "logging")]
impl<T: Transport + Sized> TransportExt for T {}
