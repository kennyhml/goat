use crate::{AdtUri, CategoryId, EntityTag, NegotiableMediaVersion, ResponseError};

pub(crate) mod private {
    pub trait Sealed {}
}

/// A typed object reference whose properties can be fetched.
#[doc(hidden)]
pub trait ObjectProperties: private::Sealed + Clone + Send + Sync + Sized {
    type MediaVersion: NegotiableMediaVersion;
    type Representation: TryFrom<RawObjectProperties<Self>, Error = Self::Error> + Send;
    type Error: Into<ResponseError>;

    const CATEGORY: CategoryId;

    fn properties_uri(&self) -> &AdtUri;
}

/// An object-properties response ready for domain-specific decoding.
#[doc(hidden)]
pub struct RawObjectProperties<R>
where
    R: ObjectProperties,
{
    pub resource: R,
    pub version: R::MediaVersion,
    pub body: Vec<u8>,
    pub etag: Option<EntityTag>,
}
