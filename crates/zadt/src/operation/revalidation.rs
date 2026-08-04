//! HTTP cache revalidation based on entity tags.

use http::StatusCode;

use super::{Operation, OperationResponse};
use crate::{AdtRequest, Client, ClientState, EntityTag, OperationError, ResponseError};

/// The outcome of a request using a cache validator such as `If-None-Match`.
#[derive(Clone, Debug)]
pub enum Revalidation<T> {
    /// The resource changed and a new representation was returned.
    Modified(T),

    /// The supplied validator still identifies the current representation.
    NotModified { etag: Option<EntityTag> },
}

/// An operation carrying an `If-None-Match` cache validator.
#[derive(Debug)]
pub struct IfNoneMatch<O> {
    inner: O,
    etag: EntityTag,
}

impl<O> IfNoneMatch<O> {
    pub(crate) fn new(inner: O, etag: EntityTag) -> Self {
        Self { inner, etag }
    }
}

impl<S, O> Operation<S> for IfNoneMatch<O>
where
    S: ClientState,
    O: Operation<S>,
{
    type Response = Revalidation<O::Response>;
    type Kind = O::Kind;

    fn request(&self, client: &Client<S>) -> Result<AdtRequest, OperationError> {
        let mut request = self.inner.request(client)?;
        request.set_cache_revalidation(Some(&self.etag));
        Ok(request)
    }

    fn decode(&self, response: OperationResponse) -> Result<Self::Response, ResponseError> {
        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok(Revalidation::NotModified {
                etag: response.entity_tag(),
            });
        }

        self.inner.decode(response).map(Revalidation::Modified)
    }
}

impl<T> Revalidation<T> {
    /// Borrows the returned representation when the resource was modified.
    pub fn as_modified(&self) -> Option<&T> {
        match self {
            Self::Modified(value) => Some(value),
            Self::NotModified { .. } => None,
        }
    }

    /// Consumes the outcome and returns the representation when modified.
    pub fn into_modified(self) -> Option<T> {
        match self {
            Self::Modified(value) => Some(value),
            Self::NotModified { .. } => None,
        }
    }

    /// Returns the ETag supplied with a not-modified response.
    pub fn not_modified_etag(&self) -> Option<&str> {
        match self {
            Self::Modified(_) => None,
            Self::NotModified { etag } => etag.as_deref(),
        }
    }
}
