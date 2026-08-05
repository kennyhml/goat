use http::{Method, StatusCode, header};

use crate::{
    client::{Client, Ready},
    compatibility::MediaVersionNegotiation,
    error::{OperationError, ResponseError},
    objects::{ObjectProperties, ObjectRef, ObjectVersion},
    operation::{IfNoneMatch, Operation, OperationResponse, Stateless},
    protocol::{AdtRequest, EntityTag},
    target::CollectionTarget,
    vocabulary::query_parameter,
};

/// Fetches a versioned ADT object-properties representation.
///
/// The operation uses the target resources endpoint and is generic over `T`
/// so it can negotiate and parse that resource's properties representation.
#[derive(Debug)]
pub struct ObjectPropertiesQuery<T>
where
    T: ObjectProperties,
{
    /// The typed object reference whose properties will be fetched.
    pub resource: ObjectRef<T>,

    /// Media-type versions in descending caller preference.
    pub priority: Vec<T::MediaVersion>,

    /// The repository-object version to request.
    pub version: Option<ObjectVersion>,
}

impl<T> ObjectPropertiesQuery<T>
where
    T: ObjectProperties,
{
    /// Creates an unconditional properties query using the profile's default priority.
    pub fn new(resource: ObjectRef<T>) -> Self {
        Self {
            resource,
            priority: T::MediaVersion::SUPPORTED.to_vec(),
            version: None,
        }
    }

    /// Replaces the media-type preference order.
    pub fn priority(mut self, priority: impl Into<Vec<T::MediaVersion>>) -> Self {
        self.priority = priority.into();
        self
    }

    /// Selects the repository-object version to request.
    pub fn version(mut self, version: ObjectVersion) -> Self {
        self.version = Some(version);
        self
    }

    /// Makes this query conditional on the supplied properties ETag.
    ///
    /// This setter must be called last.
    pub fn if_none_match(self, etag: EntityTag) -> IfNoneMatch<Self> {
        IfNoneMatch::new(self, etag)
    }
}

impl<T> Operation<Ready> for ObjectPropertiesQuery<T>
where
    T: ObjectProperties,
{
    type Response = T::Properties;
    type Kind = Stateless;

    fn request(&self, client: &Client<Ready>) -> Result<AdtRequest, OperationError> {
        let collection = CollectionTarget::new(T::CATEGORY).collection(client)?;
        let accept = crate::negotiate(&self.priority, collection.accepted_media_types())?;

        let mut request = AdtRequest::new(Method::GET, self.resource.uri().clone());
        if let Some(version) = self.version {
            request.push_query(query_parameter::VERSION, version.as_str());
        }
        request.set_accept(accept.media_type());
        request.set_cache_revalidation(None);
        Ok(request)
    }

    fn decode(&self, response: OperationResponse) -> Result<Self::Response, ResponseError> {
        if response.status() == StatusCode::NOT_MODIFIED {
            return Err(ResponseError::UnexpectedNotModified);
        }
        if response.status() != StatusCode::OK {
            return Err(ResponseError::UnexpectedStatus {
                status: response.status(),
                body: String::from_utf8_lossy(response.body()).into_owned(),
            });
        }

        let Some(content_type) = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
        else {
            return Err(ResponseError::MissingContentType {
                category: T::CATEGORY,
            });
        };
        let Some(media_version) = T::MediaVersion::from_media_type(content_type) else {
            return Err(ResponseError::UnsupportedContentType {
                category: T::CATEGORY,
                content_type: content_type.to_owned(),
                supported: T::MediaVersion::SUPPORTED
                    .iter()
                    .map(|version| version.media_type().to_owned())
                    .collect(),
            });
        };

        let etag = response.entity_tag();
        let properties = self
            .resource
            .parse(media_version, response.into_body(), etag)?;
        Ok(properties)
    }
}

impl<T: ObjectProperties> ObjectRef<T> {
    fn parse(
        &self,
        version: T::MediaVersion,
        body: Vec<u8>,
        etag: Option<EntityTag>,
    ) -> Result<T::Properties, ResponseError> {
        T::parse(self, version, body, etag)
    }

    /// Creates a property query for this object
    pub fn query(&self) -> ObjectPropertiesQuery<T> {
        ObjectPropertiesQuery::new(self.clone())
    }
}
