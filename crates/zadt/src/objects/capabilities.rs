use super::{ObjectCollection, ObjectRef, ObjectType};
use crate::{compatibility::MediaVersionNegotiation, error::ResponseError, protocol::EntityTag};

/// A statically known source component exposed by an object family.
pub trait SourceComponent: Sync {
    /// Returns the component name used by ADT.
    fn name(&self) -> &'static str;

    /// Returns the component path relative to its owning object.
    fn path(&self) -> &'static [&'static str];

    /// Returns whether this is the conventional source component.
    fn is_primary(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MainSource;

impl SourceComponent for MainSource {
    fn name(&self) -> &'static str {
        "main"
    }

    fn path(&self) -> &'static [&'static str] {
        &["source", "main"]
    }

    fn is_primary(&self) -> bool {
        true
    }
}

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

/// Annotates an object type that has a primary source component.
///
/// Implementors must include exactly one primary component in
/// [`ObjectType::SOURCE_COMPONENTS`].
pub trait Source: ObjectType {}
