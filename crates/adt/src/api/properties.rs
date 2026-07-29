use http::{Method, StatusCode, header};

use crate::{
    client::{Client, Discovered},
    compatibility::{CompatibilityError, NegotiableMediaVersion},
    error::{OperationError, ResponseError},
    objects::{ObjectProperties, ObjectRef, ObjectVersion},
    operation::{IfNoneMatch, Operation, QueryMode, Stateless, Unconditional},
    protocol::{AdtRequest, AdtResponse, EntityTag},
    vocabulary::query_parameter,
};

/// Fetches a versioned ADT object-properties representation.
#[derive(Debug)]
pub struct ObjectPropertiesQuery<T, M = Unconditional>
where
    T: ObjectProperties,
{
    /// The typed object reference whose properties will be fetched.
    pub resource: ObjectRef<T>,

    /// Media-type versions in descending caller preference.
    pub priority: Vec<T::MediaVersion>,

    /// The repository-object version to request.
    pub version: Option<ObjectVersion>,

    mode: M,
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
            mode: Unconditional,
        }
    }
}

impl<T, M> ObjectPropertiesQuery<T, M>
where
    T: ObjectProperties,
{
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
}

impl<T> ObjectPropertiesQuery<T, Unconditional>
where
    T: ObjectProperties,
{
    /// Makes this query conditional on the supplied properties ETag.
    pub fn if_none_match(self, etag: EntityTag) -> ObjectPropertiesQuery<T, IfNoneMatch> {
        ObjectPropertiesQuery {
            resource: self.resource,
            priority: self.priority,
            version: self.version,
            mode: IfNoneMatch { etag },
        }
    }
}

impl<T, M> Operation<Discovered> for ObjectPropertiesQuery<T, M>
where
    T: ObjectProperties,
    M: QueryMode<T::Properties>,
{
    type Response = M::Response;
    type Kind = Stateless;

    fn request(&self, client: &Client<Discovered>) -> Result<AdtRequest, OperationError> {
        let collection = client
            .collection(T::CATEGORY)
            .ok_or(CompatibilityError::MissingCollection(T::CATEGORY))?;
        let accept = crate::negotiate(&self.priority, collection.accepted_media_types())?;

        let mut request = AdtRequest::new(Method::GET, self.resource.uri().clone());
        if let Some(version) = self.version {
            request.push_query(query_parameter::VERSION, version.as_str());
        }
        request.set_accept(accept.media_type());
        request.set_cache_revalidation(self.mode.if_none_match());
        Ok(request)
    }

    fn decode(&self, response: AdtResponse) -> Result<Self::Response, ResponseError> {
        if response.status() == StatusCode::NOT_MODIFIED {
            return self
                .mode
                .not_modified(response.entity_tag())
                .ok_or(ResponseError::UnexpectedNotModified);
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
        Ok(self.mode.modified(properties))
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

    /// Creates an unconditional query for this object's properties.
    pub fn query(&self) -> ObjectPropertiesQuery<T> {
        ObjectPropertiesQuery::new(self.clone())
    }
}
