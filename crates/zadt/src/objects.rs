use std::{any::Any, fmt, hash::Hash, marker::PhantomData};

use crate::{
    AccessMode, LockHandle, LockRequest, UnlockRequest,
    client::{Client, Ready},
    error::ObjectError,
    resource::SourceRef,
    uri::AdtUri,
    vocabulary::CategoryId,
};

mod capabilities;
mod families;
mod policies;
mod version;
mod workbench;

pub(crate) use capabilities::MainSource;
pub use capabilities::{ObjectProperties, Source, SourceComponent};
pub use families::{Class, ClassSourceComponent, Include, Package, Program};
pub use policies::ObjectNamePolicy;
pub use version::ObjectVersion;
pub use workbench::{GlobalWorkbenchType, InvalidWorkbenchType};

pub(crate) mod private {
    pub trait Sealed {}
}

/// Statically identified ADT object resource family.
///
/// This allows object types to automatically implement various traits while
/// keeping their protocol location separate in [`ObjectCollection`].
pub trait ObjectType: private::Sealed + Send + Sync + Sized + 'static {
    /// The objects global Workbench type.
    const WORKBENCH_TYPE: GlobalWorkbenchType;

    /// The objects naming constraints.
    const NAMING_POLICY: ObjectNamePolicy;

    /// Source components with statically known paths relative to the object.
    const SOURCE_COMPONENTS: &'static [&'static dyn SourceComponent] = &[];
}

/// An object type whose canonical collection is advertised through discovery.
pub trait ObjectCollection: ObjectType {
    /// The stable category identifying the canonical object collection.
    const CATEGORY: CategoryId;
}

/// Type erased operations and metadata shared by repository objects.
///
/// While [`ObjectRef<T>`] is incredible valueable for compile time
/// static checks, it only works in scenarios where a concrete type
/// is proven, such as `fn delete_includes(prog: ObjectRef<Program>)`.
///
/// In consumer scenarios however, this is rare. Take a CLI tool that provides
/// a command such as `zg show --name Z_PROG --type PROG/P`. Dynamic dispatch
/// to the correct implementation is simply unavoidable at that point, because
/// the alternative is an amalgamation of hundreds of objects into one enum.
///
/// The overhead of a dynamic dispatch is more than negligible in comparison
/// to the network communication, but it turns out less type-safe as we can
/// only make a guarantee about very few operations, such as lifecycles, that
/// all objects support.
pub trait RepositoryObject: Any + Send + Sync {
    /// Returns the naming constraints for this object type.
    fn naming_policy(&self) -> ObjectNamePolicy;

    /// Returns the object's global Workbench type.
    fn workbench_type(&self) -> GlobalWorkbenchType;

    /// Returns the source components advertised by this object type.
    fn source_components(&self) -> &'static [&'static dyn SourceComponent];

    /// Resolves the conventional source component when one is available.
    fn source(&self) -> Option<SourceRef>;

    /// Resolves a named source component when one is available.
    fn source_component(&self, name: &str) -> Option<SourceRef>;

    /// Creates an object-lock operation.
    fn lock(&self, access_mode: AccessMode) -> LockRequest;

    /// Creates an operation that releases this object's lock.
    fn unlock(&self, lock_handle: LockHandle) -> Result<UnlockRequest, ObjectError>;
}

impl<T> RepositoryObject for ObjectRef<T>
where
    T: ObjectType,
{
    fn naming_policy(&self) -> ObjectNamePolicy {
        T::NAMING_POLICY
    }

    fn workbench_type(&self) -> GlobalWorkbenchType {
        T::WORKBENCH_TYPE
    }

    fn source_components(&self) -> &'static [&'static dyn SourceComponent] {
        T::SOURCE_COMPONENTS
    }

    fn source(&self) -> Option<SourceRef> {
        T::SOURCE_COMPONENTS
            .iter()
            .copied()
            .find(|component| component.is_primary())
            .map(|component| self.source_from_component(component))
    }

    fn source_component(&self, name: &str) -> Option<SourceRef> {
        T::SOURCE_COMPONENTS
            .iter()
            .copied()
            .find(|component| component.name() == name)
            .map(|component| self.source_from_component(component))
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
/// location. [`Client::object`] returns `ObjectRef<T>` for a known
/// [`ObjectCollection`].
pub struct ObjectRef<T = ()> {
    name: String,
    uri: AdtUri,
    marker: PhantomData<fn() -> T>,
}

impl ObjectRef {
    /// Creates a type-erased object reference from a validated ADT resource URI.
    pub(crate) fn new(uri: AdtUri) -> Self {
        Self::typed(String::new(), uri)
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
        ObjectRef::typed(self.name.clone(), self.uri.clone())
    }

    pub(crate) fn source_from_component<C>(&self, component: &C) -> SourceRef
    where
        C: SourceComponent + ?Sized,
    {
        let uri = self
            .uri()
            .append_segments(component.path())
            .expect("static source component path forms a valid ADT URI");
        SourceRef::new(self.erase(), uri)
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

impl Client<Ready> {
    /// Resolves a typed object reference from its statically known collection.
    ///
    /// Constructing a reference performs no request; the collection URI comes
    /// from the capabilities already retained by the ready client.
    pub fn object<T: ObjectCollection>(&self, name: &str) -> Result<ObjectRef<T>, ObjectError> {
        T::NAMING_POLICY.validate(name)?;
        let name = name.to_ascii_uppercase();
        let uri_name = name.to_ascii_lowercase();
        let collection = self.require_collection(T::CATEGORY)?;
        let uri = collection.target().append_segments([&uri_name])?;
        Ok(ObjectRef::typed(name, uri))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn repository_object_exposes_source_components_at_runtime() {
        let class = ObjectRef::<Class>::for_test(
            "ZCL_TEST",
            AdtUri::parse("/sap/bc/adt/oo/classes/zcl_test").unwrap(),
        );
        let object: &dyn RepositoryObject = &class;

        assert_eq!(
            object
                .source_components()
                .iter()
                .map(|component| component.name())
                .collect::<Vec<_>>(),
            [
                "main",
                "definitions",
                "implementations",
                "macros",
                "testclasses",
            ]
        );
        assert_eq!(
            object.source(),
            Some(class.component_source(ClassSourceComponent::Main))
        );
        assert_eq!(
            object.source_component("definitions"),
            Some(class.component_source(ClassSourceComponent::Definitions))
        );
        assert!(object.source_component("unknown").is_none());

        let package = ObjectRef::<Package>::for_test(
            "ZPACKAGE",
            AdtUri::parse("/sap/bc/adt/packages/zpackage").unwrap(),
        );
        let object: &dyn RepositoryObject = &package;

        assert!(object.source_components().is_empty());
        assert!(object.source().is_none());
        assert!(object.source_component("main").is_none());
    }
}
