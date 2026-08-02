//! Conditional operation modes based on HTTP entity tags.

use crate::EntityTag;

mod private {
    pub trait Sealed {}
}

/// The outcome of a request using a cache validator such as `If-None-Match`.
#[derive(Clone, Debug)]
pub enum Conditional<T> {
    /// The resource changed and a new representation was returned.
    Modified(T),

    /// The supplied validator still identifies the current representation.
    NotModified { etag: Option<EntityTag> },
}

impl<T> Conditional<T> {
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

/// Marker for a query without an HTTP cache validator.
#[derive(Clone, Copy, Debug, Default)]
pub struct Unconditional;

/// Marker for a query carrying an `If-None-Match` validator.
#[derive(Clone, Debug)]
pub struct IfNoneMatch {
    pub(crate) etag: EntityTag,
}

impl private::Sealed for Unconditional {}
impl private::Sealed for IfNoneMatch {}

/// Selects the response produced by a cache-aware query mode.
///
/// This trait is sealed; use a query's `if_none_match` method to change modes.
#[doc(hidden)]
pub trait QueryMode<V>: private::Sealed + Send + Sync {
    type Response: Send;

    fn if_none_match(&self) -> Option<&EntityTag>;
    fn modified(&self, value: V) -> Self::Response;
    fn not_modified(&self, etag: Option<EntityTag>) -> Option<Self::Response>;
}

impl<V: Send> QueryMode<V> for Unconditional {
    type Response = V;

    fn if_none_match(&self) -> Option<&EntityTag> {
        None
    }

    fn modified(&self, value: V) -> Self::Response {
        value
    }

    fn not_modified(&self, _etag: Option<EntityTag>) -> Option<Self::Response> {
        None
    }
}

impl<V: Send> QueryMode<V> for IfNoneMatch {
    type Response = Conditional<V>;

    fn if_none_match(&self) -> Option<&EntityTag> {
        Some(&self.etag)
    }

    fn modified(&self, value: V) -> Self::Response {
        Conditional::Modified(value)
    }

    fn not_modified(&self, etag: Option<EntityTag>) -> Option<Self::Response> {
        Some(Conditional::NotModified { etag })
    }
}
