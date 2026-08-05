use super::{ObjectCollection, ObjectRef, ObjectType};
use crate::{compatibility::MediaVersionNegotiation, error::ResponseError, protocol::EntityTag};

/// Annotates an object type that supports fetching and decoding properties.
#[doc(hidden)]
pub trait ObjectProperties: ObjectCollection {
    type MediaVersion: MediaVersionNegotiation;
    type Properties: Send;

    fn parse(
        resource: &ObjectRef<Self>,
        version: Self::MediaVersion,
        body: Vec<u8>,
        etag: Option<EntityTag>,
    ) -> Result<Self::Properties, ResponseError>;
}

/// Annotates an object type that has source code that can be read.
///
/// The source code is usually located at `/source/main` outgoing
/// from the source object.
pub trait Source: ObjectType {
    const PATH: &'static [&'static str] = &["source", "main"];
}
