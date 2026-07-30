use std::{any::Any, fmt, hash::Hash, marker::PhantomData};

use url::Url;

use crate::{
    AccessMode, LockHandle, LockRequest, UnlockRequest,
    client::{Client, Discovered},
    compatibility::CompatibilityError,
    error::ObjectError,
    uri::{AdtUri, AdtUriError},
    vocabulary::CategoryId,
};

mod capabilities;
mod policies;
mod profiles;
mod version;
mod workbench;

pub use capabilities::{ObjectProperties, Source};
pub use policies::ObjectNamePolicy;
pub use profiles::{Include, Package, Program};
pub use version::ObjectVersion;
pub use workbench::{GlobalWorkbenchType, InvalidWorkbenchType};

pub(crate) mod private {
    pub trait Sealed {}
}

/// Statically identified ADT object resource family.
///
/// This allows object types to automatically implement various traits
/// and enables the objects capabilities to be located dynamically by
/// following the category in the discovery.
pub trait ObjectType: private::Sealed + Send + Sync + Sized + 'static {
    /// The category to identify the objects profile in a collection
    const CATEGORY: CategoryId;

    /// The objects global Workbench type.
    const WORKBENCH_TYPE: GlobalWorkbenchType;

    /// The objects naming constraints.
    const NAMING_POLICY: ObjectNamePolicy;
}

/// Type-erased operations and metadata shared by repository objects.
pub trait RepositoryObject: Any + Send + Sync {
    /// Returns the discovery category for this object type.
    fn category(&self) -> CategoryId;

    /// Returns the naming constraints for this object type.
    fn naming_policy(&self) -> ObjectNamePolicy;

    /// Returns the object's global Workbench type.
    fn workbench_type(&self) -> GlobalWorkbenchType;

    /// Creates an object-lock operation.
    fn lock(&self, access_mode: AccessMode) -> LockRequest;

    /// Creates an operation that releases this object's lock.
    fn unlock(&self, lock_handle: LockHandle) -> Result<UnlockRequest, ObjectError>;
}

impl<T> RepositoryObject for ObjectRef<T>
where
    T: ObjectType,
{
    fn category(&self) -> CategoryId {
        T::CATEGORY
    }

    fn naming_policy(&self) -> ObjectNamePolicy {
        T::NAMING_POLICY
    }

    fn workbench_type(&self) -> GlobalWorkbenchType {
        T::WORKBENCH_TYPE
    }

    fn lock(&self, access_mode: AccessMode) -> LockRequest {
        ObjectRef::<T>::lock(self, access_mode)
    }

    fn unlock(&self, lock_handle: LockHandle) -> Result<UnlockRequest, ObjectError> {
        ObjectRef::<T>::unlock(self, lock_handle)
    }
}

impl dyn RepositoryObject + '_ {
    /// Attempts to recover a statically typed object reference.
    pub fn downcast_ref<T: ObjectType>(&self) -> Option<&ObjectRef<T>> {
        let any: &dyn Any = self;
        any.downcast_ref()
    }
}

/// A validated ADT object identity, optionally tagged with its static object type.
///
/// A bare `ObjectRef` is type-erased and proves only the objects identity and
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

    pub(crate) fn from_parts(name: String, uri: AdtUri) -> Result<Self, ObjectError> {
        T::NAMING_POLICY.validate(&name)?;

        // TODO: Dont always uppercase!
        Ok(Self::typed(name.to_ascii_uppercase(), uri))
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

impl Client<Discovered> {
    /// Resolves a typed object reference from its statically known collection.
    ///
    /// Constructing a reference performs no request; the collection URI comes
    /// from the capabilities already retained by the discovered client.
    pub fn object<T: ObjectType>(&self, name: &str) -> Result<ObjectRef<T>, ObjectError> {
        T::NAMING_POLICY.validate(name)?;
        let name = name.to_ascii_uppercase();
        let uri_name = name.to_ascii_lowercase();
        let collection = self
            .collection(T::CATEGORY)
            .ok_or(CompatibilityError::MissingCollection(T::CATEGORY))?;
        let uri = append_segments(collection.target(), [&uri_name])?;
        Ok(ObjectRef::typed(name, uri))
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_workbench_types_use_unpadded_sap_field_limits() {
        let object_type = GlobalWorkbenchType::new("ABCD", "XYZ");

        assert_eq!(object_type.directory_type(), "ABCD");
        assert_eq!(object_type.workbench_type(), "XYZ");
        assert_eq!(object_type.to_string(), "ABCD/XYZ");
        assert_eq!(Program::WORKBENCH_TYPE.to_string(), "PROG/P");
        assert_eq!(Include::WORKBENCH_TYPE.to_string(), "PROG/I");
    }

    #[test]
    fn parses_an_owned_global_workbench_type() {
        let object_type: GlobalWorkbenchType = "CLAS/OM".parse().unwrap();

        assert_eq!(object_type.directory_type(), "CLAS");
        assert_eq!(object_type.workbench_type(), "OM");
        assert_eq!(object_type.to_string(), "CLAS/OM");
    }

    #[test]
    fn rejects_invalid_global_workbench_type_responses() {
        for value in [
            "CLAS",
            "/OM",
            "CLAS/",
            "CLAS/OM/X",
            "TOOLONG/X",
            "CLAS/LONG",
        ] {
            assert!(
                value.parse::<GlobalWorkbenchType>().is_err(),
                "accepted {value}"
            );
        }
    }

    #[test]
    #[should_panic(expected = "R3TR object type exceeds 4 characters")]
    fn global_workbench_type_rejects_an_oversized_directory_type() {
        GlobalWorkbenchType::new("ABCDE", "X");
    }

    #[test]
    #[should_panic(expected = "Workbench type exceeds 3 characters")]
    fn global_workbench_type_rejects_an_oversized_internal_type() {
        GlobalWorkbenchType::new("ABCD", "WXYZ");
    }

    #[test]
    fn object_name_policies_enforce_type_specific_limits() {
        assert_eq!(Program::NAMING_POLICY.maximum_length(), 30);
        assert_eq!(Include::NAMING_POLICY.maximum_length(), 40);
        assert!(Program::NAMING_POLICY.validate(&"A".repeat(30)).is_ok());
        assert!(Include::NAMING_POLICY.validate(&"A".repeat(40)).is_ok());

        let name = "A".repeat(31);
        let error = Program::NAMING_POLICY.validate(&name).unwrap_err();
        assert!(matches!(
            error,
            ObjectError::NameTooLong {
                name: rejected,
                maximum_length: 30,
            } if rejected == name
        ));
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
    fn repository_object_dispatches_locking_and_downcasts() {
        let program = ObjectRef::<Program>::for_test(
            "Z_TEST",
            AdtUri::parse("/sap/bc/adt/programs/programs/Z_TEST").unwrap(),
        );
        let object: &dyn RepositoryObject = &program;

        let request = object.lock(AccessMode::Modify);

        assert_eq!(request.object, program.erase());
        assert_eq!(object.workbench_type(), Program::WORKBENCH_TYPE);
        assert!(object.downcast_ref::<Program>().is_some());
        assert!(object.downcast_ref::<Include>().is_none());
    }
}
