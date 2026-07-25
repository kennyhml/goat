use std::fmt;

use url::Url;

use crate::{
    AccessMode, AdtUri, AdtUriError, Capabilities, LockHandle, ObjectError, ObjectLock,
    ObjectUnlock, ProgramError,
};

const PROGRAMS_SCHEME: &str = "http://www.sap.com/adt/categories/programs";
const PROGRAMS_TERM: &str = "programs";

/// A validated reference to an ADT repository object.
///
/// An object reference is an identity and resource location, not a fetched
/// object representation. It can refer to any lockable ADT object, including a
/// program, class, CDS source, or DDIC structure. Constructing one performs no
/// I/O.
///
/// From an object reference, callers can derive source resources and generic
/// operations without rebuilding URI strings:
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ObjectRef(AdtUri);

impl ObjectRef {
    /// Creates an object reference from a validated ADT resource URI.
    pub fn new(uri: AdtUri) -> Self {
        Self(uri)
    }

    /// Parses and validates an object resource URI.
    pub fn parse(value: &str) -> Result<Self, AdtUriError> {
        AdtUri::parse(value).map(Self)
    }

    /// Returns the object's resource URI.
    pub fn uri(&self) -> &AdtUri {
        &self.0
    }

    /// Creates a source reference below this object using relative path segments.
    ///
    /// Use this for nonstandard source resources, such as class test sources.
    /// Dynamic segments are encoded as individual URI path segments.
    pub fn source<I, T>(&self, segments: I) -> Result<SourceRef, AdtUriError>
    where
        I: IntoIterator<Item = T>,
        T: AsRef<str>,
    {
        append_segments(self.uri(), segments).map(|uri| SourceRef {
            object: self.clone(),
            uri,
        })
    }

    /// Returns this object's conventional `source/main` resource.
    ///
    /// The returned [`SourceRef`] remembers this object so an update builder can
    /// reject a lock obtained for a different object.
    pub fn main_source(&self) -> SourceRef {
        self.source(["source", "main"])
            .expect("static source path segments form a valid ADT URI")
    }
}

impl fmt::Display for ObjectRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl From<AdtUri> for ObjectRef {
    fn from(value: AdtUri) -> Self {
        Self::new(value)
    }
}

/// A validated source-code resource and its owning repository object.
///
/// A source URI alone does not establish which object lock authorizes an
/// update. `SourceRef` therefore retains both the source URI and its
/// [`ObjectRef`]. [`SourceRef::update`](crate::SourceRef::update) uses that
/// relationship to validate a [`LockHandle`](crate::LockHandle) at build time.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[readonly::make]
pub struct SourceRef {
    /// The object that owns this source resource.
    pub object: ObjectRef,

    /// The source resource URI.
    pub uri: AdtUri,
}

impl fmt::Display for SourceRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.uri.fmt(formatter)
    }
}

/// A program identity resolved from the programs collection in central discovery.
///
/// Unlike [`ObjectRef::parse`], [`Client::program`](crate::Client::program) does
/// not require callers to know the programs collection URI. It looks up the
/// stable programs category in the client's discovered capabilities, appends
/// the program name as one encoded path segment, and returns a reference that
/// exposes both the program object and its main source.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ProgramRef(ObjectRef);

impl ProgramRef {
    pub(crate) fn resolve(
        capabilities: &Capabilities,
        program_name: impl Into<String>,
    ) -> Result<Self, ProgramError> {
        let program_name = program_name.into();
        validate_program_name(&program_name)?;
        let collection = capabilities
            .collection(PROGRAMS_SCHEME, PROGRAMS_TERM)
            .ok_or(ProgramError::MissingCollection)?;
        let uri = append_segments(collection.target(), [&program_name])?;
        Ok(Self(ObjectRef::new(uri)))
    }

    /// Returns the program object reference.
    pub fn object(&self) -> &ObjectRef {
        &self.0
    }

    /// Creates a generic object-lock operation for this program.
    ///
    /// This forwards to [`ObjectRef::lock`] so normal program workflows do not
    /// need to access the underlying object reference explicitly.
    pub fn lock(&self, access_mode: AccessMode) -> ObjectLock {
        self.0.lock(access_mode)
    }

    /// Creates an operation that releases this program's object lock.
    pub fn unlock(&self, lock_handle: LockHandle) -> Result<ObjectUnlock, ObjectError> {
        self.0.unlock(lock_handle)
    }

    /// Returns the program object URI.
    pub fn uri(&self) -> &AdtUri {
        self.0.uri()
    }

    /// Returns the program's main source resource.
    pub fn source(&self) -> SourceRef {
        self.0.main_source()
    }
}

fn validate_program_name(program_name: &str) -> Result<(), ProgramError> {
    if program_name.is_empty()
        || program_name.trim() != program_name
        || program_name.chars().any(char::is_control)
        || matches!(program_name, "." | "..")
    {
        return Err(ProgramError::InvalidName {
            name: program_name.to_owned(),
        });
    }
    Ok(())
}

fn append_segments<I, T>(base: &AdtUri, segments: I) -> Result<AdtUri, AdtUriError>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_source_resources_from_validated_objects() {
        let object = ObjectRef::parse("/sap/bc/adt/ddic/structures/ZSTRUCTURE").unwrap();

        assert_eq!(
            object.main_source().uri.as_str(),
            "/sap/bc/adt/ddic/structures/ZSTRUCTURE/source/main"
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
}
