use std::{fmt, hash::Hash, marker::PhantomData};

use http::{Method, StatusCode, header};
use url::Url;

use crate::{
    api::object::{ObjectLock, ObjectUnlock},
    client::{Client, Discovered},
    compatibility::{CompatibilityError, NegotiableMediaVersion},
    error::{IncludeError, ObjectError, OperationError, ProgramError, ResponseError},
    models::{
        AccessMode, IncludeMediaVersion, IncludeProperties, LockHandle, ProgramMediaVersion,
        ProgramProperties,
    },
    operation::{IfNoneMatch, Operation, QueryMode, Stateless, Unconditional},
    protocol::{AdtRequest, AdtResponse, EntityTag},
    resource::SourceRef,
    uri::{AdtUri, AdtUriError},
    vocabulary::{CategoryId, INCLUDES, PROGRAMS, query_parameter},
};

pub(crate) mod private {
    pub trait Sealed {}
}

/// An ADT repository-object version accepted by the `version` query parameter.
///
/// These values are the public URI vocabulary from
/// `IF_ADT_URI_QUERY_PARAMETERS`. SAP maps them internally to one-character
/// ABAP Workbench `R3STATE` values.
///
/// # SAP references
///
/// - `IF_ADT_URI_QUERY_PARAMETERS` defines `CO_VERSION` and its external values;
/// - `CL_SEDI_ADT_RES_SOURCE->GET` reads the parameter for programs;
/// - `CL_ADT_UTILITY->GET_WB_VERSION` maps it to Workbench `R3STATE` values.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ObjectVersion {
    /// The persistent active object (R3STATE `A`).
    Active,

    /// An inactive object awaiting activation (R3STATE `I`).
    Inactive,

    /// Uses the current user's inactive version when available (R3STATE `_`).
    WorkingArea,

    /// A newly created object (R3STATE `N`).
    New,

    /// An object for which only part of the content is active (R3STATE `P`).
    PartlyActive,
}

impl ObjectVersion {
    /// Returns the exact value used by ADT URI query parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::WorkingArea => "workingArea",
            Self::New => "new",
            Self::PartlyActive => "partlyActive",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "inactive" => Some(Self::Inactive),
            "workingArea" => Some(Self::WorkingArea),
            "new" => Some(Self::New),
            "partlyActive" => Some(Self::PartlyActive),
            _ => None,
        }
    }
}

impl fmt::Display for ObjectVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Statically identifies an ADT object resource family.
pub trait ObjectType: private::Sealed + Send + Sync + Sized + 'static {
    /// The category identifying the object's collection and protocol profile.
    const CATEGORY: CategoryId;
}

/// The ABAP program object type.
#[derive(Debug)]
pub enum Program {}

impl ObjectType for Program {
    const CATEGORY: CategoryId = PROGRAMS;
}

/// The standalone ABAP include object type.
#[derive(Debug)]
pub enum Include {}

impl ObjectType for Include {
    const CATEGORY: CategoryId = INCLUDES;
}

/// A typed reference to an ABAP program.
pub type ProgramRef = ObjectRef<Program>;

/// A typed reference to a standalone ABAP include.
pub type IncludeRef = ObjectRef<Include>;

impl private::Sealed for Program {}
impl private::Sealed for Include {}

/// A validated ADT object identity, optionally tagged with its static object type.
///
/// A bare `ObjectRef` is type-erased and proves only the object's identity and
/// location. [`Client::object`] returns `ObjectRef<T>` for a known [`ObjectType`].
pub struct ObjectRef<T = ()> {
    name: String,
    uri: AdtUri,
    marker: PhantomData<fn() -> T>,
}

impl ObjectRef {
    /// Creates a type-erased object reference from a validated ADT resource URI.
    pub fn new(uri: AdtUri) -> Self {
        Self {
            name: String::new(),
            uri,
            marker: PhantomData,
        }
    }

    /// Parses and validates a type-erased object resource URI.
    pub fn parse(value: &str) -> Result<Self, AdtUriError> {
        AdtUri::parse(value).map(Self::new)
    }
}

impl<T> ObjectRef<T> {
    fn typed(name: String, uri: AdtUri) -> Self {
        Self {
            name,
            uri,
            marker: PhantomData,
        }
    }

    /// Returns the object's resource URI.
    pub fn uri(&self) -> &AdtUri {
        &self.uri
    }

    /// Returns a type-erased copy of this object identity.
    pub fn erase(&self) -> ObjectRef {
        ObjectRef {
            name: self.name.clone(),
            uri: self.uri.clone(),
            marker: PhantomData,
        }
    }
}

impl<T: ObjectType> ObjectRef<T> {
    /// Returns the canonical uppercase object name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl ObjectRef<Program> {
    #[cfg(test)]
    pub(crate) fn for_test(name: &str, uri: AdtUri) -> Self {
        Self::typed(name.to_ascii_uppercase(), uri)
    }

    /// Returns the program's conventional `source/main` resource.
    pub fn source(&self) -> SourceRef {
        conventional_source(self)
    }
}

impl ObjectRef<Include> {
    #[cfg(test)]
    pub(crate) fn for_test(name: &str, uri: AdtUri) -> Self {
        Self::typed(name.to_ascii_uppercase(), uri)
    }

    /// Returns the include's conventional `source/main` resource.
    pub fn source(&self) -> SourceRef {
        conventional_source(self)
    }
}

impl<T> Clone for ObjectRef<T> {
    fn clone(&self) -> Self {
        Self::typed(self.name.clone(), self.uri.clone())
    }
}

impl<T> fmt::Debug for ObjectRef<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("ObjectRef");
        if !self.name.is_empty() {
            debug.field("name", &self.name);
        }
        debug.field("uri", &self.uri).finish()
    }
}

impl<T> PartialEq for ObjectRef<T> {
    fn eq(&self, other: &Self) -> bool {
        self.uri == other.uri
    }
}

impl<T> Eq for ObjectRef<T> {}

impl<T> Hash for ObjectRef<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.uri.hash(state);
    }
}

impl<T> fmt::Display for ObjectRef<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.uri.fmt(formatter)
    }
}

impl From<AdtUri> for ObjectRef {
    fn from(value: AdtUri) -> Self {
        Self::new(value)
    }
}

/// A Workbench object type supporting the standard ADT lock lifecycle.
pub trait WorkbenchObject: ObjectType {}

impl WorkbenchObject for Program {}
impl WorkbenchObject for Include {}

impl<T: WorkbenchObject> ObjectRef<T> {
    /// Creates an object-lock operation.
    pub fn lock(&self, access_mode: AccessMode) -> ObjectLock {
        ObjectLock::new(self.erase(), access_mode)
    }

    /// Creates an operation that releases this object's lock.
    pub fn unlock(&self, lock_handle: LockHandle) -> Result<ObjectUnlock, ObjectError> {
        if self.uri() != lock_handle.object.uri() {
            return Err(ObjectError::LockHandleObjectMismatch {
                expected: self.to_string(),
                actual: lock_handle.object.to_string(),
            });
        }
        Ok(ObjectUnlock::new(lock_handle))
    }
}

/// Static metadata needed to fetch and decode an object's properties.
#[doc(hidden)]
pub trait ObjectProperties: ObjectType {
    type MediaVersion: NegotiableMediaVersion;
    type Representation: TryFrom<RawObjectProperties<Self>, Error = Self::Error> + Send;
    type Error: Into<ResponseError>;
}

impl ObjectProperties for Program {
    type MediaVersion = ProgramMediaVersion;
    type Representation = ProgramProperties;
    type Error = ProgramError;
}

impl ObjectProperties for Include {
    type MediaVersion = IncludeMediaVersion;
    type Representation = IncludeProperties;
    type Error = IncludeError;
}

/// An object-properties response ready for domain-specific decoding.
#[doc(hidden)]
pub struct RawObjectProperties<T>
where
    T: ObjectProperties,
{
    pub resource: ObjectRef<T>,
    pub version: T::MediaVersion,
    pub body: Vec<u8>,
    pub etag: Option<EntityTag>,
}

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
    M: QueryMode<T::Representation>,
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

        let raw = RawObjectProperties {
            resource: self.resource.clone(),
            version: media_version,
            etag: response_etag(&response),
            body: response.into_body(),
        };
        let representation = T::Representation::try_from(raw).map_err(Into::into)?;
        Ok(self.mode.modified(representation))
    }
}

impl<T: ObjectProperties> ObjectRef<T> {
    /// Creates an unconditional query for this object's properties.
    pub fn query(&self) -> ObjectPropertiesQuery<T> {
        ObjectPropertiesQuery::new(self.clone())
    }
}

impl Client<Discovered> {
    /// Resolves a typed object reference from its statically known collection.
    ///
    /// Constructing a reference performs no request; the collection URI comes
    /// from the capabilities already retained by the discovered client.
    pub fn object<T: ObjectType>(&self, name: &str) -> Result<ObjectRef<T>, ObjectError> {
        validate_object_name(name)?;
        let name = name.to_ascii_uppercase();
        let collection = self
            .collection(T::CATEGORY)
            .ok_or(CompatibilityError::MissingCollection(T::CATEGORY))?;
        let uri = append_segments(collection.target(), [&name])?;
        Ok(ObjectRef::typed(name, uri))
    }
}

fn conventional_source<T: ObjectType>(object: &ObjectRef<T>) -> SourceRef {
    let uri = append_segments(object.uri(), ["source", "main"])
        .expect("static source path segments form a valid ADT URI");
    SourceRef::new(object.erase(), uri)
}

fn validate_object_name(name: &str) -> Result<(), ObjectError> {
    if name.is_empty()
        || name.trim() != name
        || name.chars().any(char::is_control)
        || matches!(name, "." | "..")
    {
        return Err(ObjectError::InvalidName {
            name: name.to_owned(),
        });
    }
    Ok(())
}

pub(crate) fn append_segments<I, S>(base: &AdtUri, segments: I) -> Result<AdtUri, AdtUriError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut url = Url::parse(&format!("https://adt.invalid{}", base.as_str()))
        .expect("a validated root-relative ADT URI forms a valid URL");
    url.path_segments_mut()
        .expect("an HTTP URL supports path segments")
        .extend(
            segments
                .into_iter()
                .map(|segment| segment.as_ref().to_owned()),
        );
    AdtUri::parse(url.path())
}

fn response_etag(response: &AdtResponse) -> Option<EntityTag> {
    response
        .headers()
        .get(header::ETAG)
        .and_then(EntityTag::from_header_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_the_conventional_source_from_a_program_reference() {
        let program = ObjectRef::<Program>::for_test(
            "ZPROGRAM",
            AdtUri::parse("/sap/bc/adt/programs/programs/ZPROGRAM").unwrap(),
        );

        assert_eq!(
            program.source().uri.as_str(),
            "/sap/bc/adt/programs/programs/ZPROGRAM/source/main"
        );
    }

    #[test]
    fn encodes_dynamic_names_as_single_path_segments() {
        let collection = AdtUri::parse("/sap/bc/adt/programs/programs").unwrap();

        assert_eq!(
            append_segments(&collection, ["/DMO/PROGRAM"])
                .unwrap()
                .as_str(),
            "/sap/bc/adt/programs/programs/%2FDMO%2FPROGRAM"
        );
    }

    #[test]
    fn object_versions_use_the_adt_query_parameter_vocabulary() {
        for (version, value) in [
            (ObjectVersion::Active, "active"),
            (ObjectVersion::Inactive, "inactive"),
            (ObjectVersion::WorkingArea, "workingArea"),
            (ObjectVersion::New, "new"),
            (ObjectVersion::PartlyActive, "partlyActive"),
        ] {
            assert_eq!(version.as_str(), value);
            assert_eq!(ObjectVersion::parse(value), Some(version));
        }
    }
}
