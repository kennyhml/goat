use http::{Method, StatusCode};

use crate::{
    AdtRequest, Capabilities, Client, ClientState, Initial, Operation, OperationError,
    OperationResponse, Ready, ResponseError, Stateless,
    models::parse_capabilities,
    target::{CENTRAL_DISCOVERY, CORE_DISCOVERY},
    vocabulary::media_type,
};

/// Fetches the small, fixed ADT bootstrap capability document.
///
/// `GET /sap/bc/adt/core/discovery` returns an AtomPub service document with
/// infrastructure collections such as compatibility and batch resources. It
/// is distinct from [`DiscoveryQuery`], which advertises the domain workspaces
/// and collections used by most ADT operations. [`Client::discover`] stores
/// both documents in the resulting [`Ready`] client.
///
/// The endpoint is known in advance, so this operation can execute with any
/// [`ClientState`]. Executing it returns [`Capabilities`] but does not perform
/// the [`Client::discover`] typestate transition.
///
/// # Observed server handlers
///
/// SAP's ADT development diagnostics map this endpoint to:
///
/// - `CL_ADT_DISCOVERY_BASE_RES_APP->REGISTER_RESOURCES` for registration;
/// - `CL_ADT_RES_DISCOVERY_BASE->GET` for the `GET` implementation.
#[derive(Debug, Default)]
pub struct CoreDiscoveryQuery;

impl<S: ClientState> Operation<S> for CoreDiscoveryQuery {
    type Response = Capabilities;
    type Kind = Stateless;

    fn request(&self, _client: &Client<S>) -> Result<AdtRequest, OperationError> {
        let mut request = CORE_DISCOVERY.request(Method::GET);
        request.set_accept(media_type::DISCOVERY);
        Ok(request)
    }

    fn decode(&self, response: OperationResponse) -> Result<Self::Response, ResponseError> {
        decode_discovery_response(response)
    }
}

/// Fetches the central ADT capability document.
///
/// `GET /sap/bc/adt/discovery` returns an AtomPub service document containing
/// the server's domain workspaces and collections. Each collection can
/// advertise its resource URI, stable category identities, accepted media
/// types, and URI template links.
///
/// The endpoint is known in advance, so this operation can execute with any
/// [`ClientState`]. [`Client::discover`] executes it and stores the resulting
/// [`Capabilities`] while transitioning to [`Ready`].
///
/// # Observed server handlers
///
/// SAP's ADT development diagnostics map this endpoint to:
///
/// - `CL_ADT_DISCOVERY_RES_APP->REGISTER_RESOURCES` for registration;
/// - `CL_ADT_RES_DISCOVERY->GET` for the `GET` implementation.
#[derive(Debug, Default)]
pub struct DiscoveryQuery;

impl<S: ClientState> Operation<S> for DiscoveryQuery {
    type Response = Capabilities;
    type Kind = Stateless;

    fn request(&self, _client: &Client<S>) -> Result<AdtRequest, OperationError> {
        let mut request = CENTRAL_DISCOVERY.request(Method::GET);
        request.set_accept(media_type::DISCOVERY);
        Ok(request)
    }

    fn decode(&self, response: OperationResponse) -> Result<Self::Response, ResponseError> {
        decode_discovery_response(response)
    }
}

impl Client<Initial> {
    /// Fetches central and core ADT discovery and returns a ready client.
    pub async fn discover(self) -> Result<Client<Ready>, OperationError> {
        let capabilities = DiscoveryQuery.execute(&self).await?;
        let core_capabilities = CoreDiscoveryQuery.execute(&self).await?;
        Ok(self.with_capabilities(capabilities, core_capabilities))
    }
}

// Both dicovery endpoints use the same format
fn decode_discovery_response(response: OperationResponse) -> Result<Capabilities, ResponseError> {
    if response.status() != StatusCode::OK {
        return Err(ResponseError::UnexpectedStatus {
            status: response.status(),
            body: String::from_utf8_lossy(response.body()).into_owned(),
        });
    }

    parse_capabilities(response.body()).map_err(ResponseError::from)
}
