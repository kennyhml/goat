use super::{ObjectRef, ObjectType};
use crate::{compatibility::NegotiableMediaVersion, error::ResponseError, protocol::EntityTag};

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

/// Annotates an object type that has source code that can be read.
///
/// The source code is usually located at `/source/main` outgoing
/// from the source object.
pub trait Source: ObjectType {
    const PATH: &'static [&'static str] = &["source", "main"];
}
