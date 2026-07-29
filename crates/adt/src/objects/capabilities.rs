use super::{ObjectRef, ObjectType};
use crate::{compatibility::NegotiableMediaVersion, error::ResponseError, protocol::EntityTag};

/// Annotates an object type that supports locking and consequently also
/// unlocking operations.
///
/// The created lock handle is bound to the [`crate::ObjectRef`] it was created
/// for. Trying to remove a lock from another object fails before a request is
/// made.
pub trait Lock: ObjectType {}

/// Annotates an object type that supports fetching and decoding properties.
#[doc(hidden)]
pub trait ObjectProperties: ObjectType {
    type MediaVersion: NegotiableMediaVersion;
    type Properties: Send;

    fn parse(
        resource: &ObjectRef<Self>,
        version: Self::MediaVersion,
        body: Vec<u8>,
        etag: Option<EntityTag>,
    ) -> Result<Self::Properties, ResponseError>;
}
