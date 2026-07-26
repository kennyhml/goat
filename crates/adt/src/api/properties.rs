use http::{Method, StatusCode, header};

use crate::{
    AdtRequest, AdtResponse, Client, CompatibilityError, Discovered, EntityTag,
    NegotiableMediaVersion, ObjectProperties, ObjectVersion, Operation, OperationError, QueryMode,
    RawObjectProperties, ResponseError, Stateless, Unconditional, operation::IfNoneMatch,
    vocabulary::query_parameter,
};

/// Fetches a versioned ADT object-properties representation.
#[derive(Debug)]
pub struct ObjectPropertiesQuery<R, M = Unconditional>
where
    R: ObjectProperties,
{
    /// The object-properties resource to fetch.
    pub resource: R,

    /// Media-type versions in descending caller preference.
    pub priority: Vec<R::MediaVersion>,

    /// The repository-object version to request.
    pub version: Option<ObjectVersion>,

    mode: M,
}

impl<R> ObjectPropertiesQuery<R>
where
    R: ObjectProperties,
{
    /// Creates an unconditional properties query using the profile's default priority.
    pub fn new(resource: R) -> Self {
        Self {
            resource,
            priority: R::MediaVersion::SUPPORTED.to_vec(),
            version: None,
            mode: Unconditional,
        }
    }
}

impl<R, M> ObjectPropertiesQuery<R, M>
where
    R: ObjectProperties,
{
    /// Replaces the media-type preference order.
    pub fn priority(mut self, priority: impl Into<Vec<R::MediaVersion>>) -> Self {
        self.priority = priority.into();
        self
    }

    /// Selects the repository-object version to request.
    pub fn version(mut self, version: ObjectVersion) -> Self {
        self.version = Some(version);
        self
    }
}

impl<R> ObjectPropertiesQuery<R, Unconditional>
where
    R: ObjectProperties,
{
    /// Makes this query conditional on the supplied properties ETag.
    pub fn if_none_match(self, etag: EntityTag) -> ObjectPropertiesQuery<R, IfNoneMatch> {
        ObjectPropertiesQuery {
            resource: self.resource,
            priority: self.priority,
            version: self.version,
            mode: IfNoneMatch { etag },
        }
    }
}

impl<R, M> Operation<Discovered> for ObjectPropertiesQuery<R, M>
where
    R: ObjectProperties,
    M: QueryMode<R::Representation>,
{
    type Response = M::Response;
    type Kind = Stateless;

    fn request(&self, client: &Client<Discovered>) -> Result<AdtRequest, OperationError> {
        let collection = client
            .collection(R::CATEGORY)
            .ok_or(CompatibilityError::MissingCollection(R::CATEGORY))?;

        let accept = crate::negotiate(&self.priority, collection.accepted_media_types())?;

        let mut request = AdtRequest::new(Method::GET, self.resource.properties_uri().clone());
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
                .not_modified(response_etag(&response))
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
                category: R::CATEGORY,
            });
        };
        let Some(media_version) = R::MediaVersion::from_media_type(content_type) else {
            return Err(ResponseError::UnsupportedContentType {
                category: R::CATEGORY,
                content_type: content_type.to_owned(),
                supported: R::MediaVersion::SUPPORTED
                    .iter()
                    .map(|version| version.media_type().to_owned())
                    .collect(),
            });
        };

        let raw = RawObjectProperties {
            resource: self.resource.clone(),
            version: media_version,
            etag: response_etag(&response),
            body: response.into_body(),
        };
        let representation = R::Representation::try_from(raw).map_err(Into::into)?;
        Ok(self.mode.modified(representation))
    }
}

fn response_etag(response: &AdtResponse) -> Option<EntityTag> {
    response
        .headers()
        .get(header::ETAG)
        .and_then(EntityTag::from_header_value)
}
