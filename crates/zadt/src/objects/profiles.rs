use super::{
    GlobalWorkbenchType, ObjectCollection, ObjectNamePolicy, ObjectProperties, ObjectRef,
    ObjectType, Source, private,
};
use crate::{
    error::ResponseError,
    models::{
        IncludeProperties, IncludePropertyVersion, PackageProperties, PackagePropertiesVersion,
        ProgramProperties, ProgramPropertiesVersion,
    },
    protocol::EntityTag,
    resource::SourceRef,
    vocabulary::{CLASSES, CategoryId, INCLUDES, PROGRAMS},
};

impl private::Sealed for Package {}
impl private::Sealed for Program {}
impl private::Sealed for Include {}
impl private::Sealed for Class {}

/// The package (devclass) object type.
#[derive(Debug)]
pub enum Package {}

impl ObjectType for Package {
    const WORKBENCH_TYPE: GlobalWorkbenchType = GlobalWorkbenchType::new("DEVC", "K");
    const NAMING_POLICY: ObjectNamePolicy = ObjectNamePolicy::new(30);
}

impl ObjectCollection for Package {
    const CATEGORY: CategoryId = CategoryId {
        scheme: "http://www.sap.com/wbobj/packages",
        term: "devck",
    };
}

impl ObjectProperties for Package {
    type MediaVersion = PackagePropertiesVersion;
    type Properties = PackageProperties;

    fn parse(
        resource: &ObjectRef<Self>,
        version: Self::MediaVersion,
        body: Vec<u8>,
        etag: Option<EntityTag>,
    ) -> Result<Self::Properties, ResponseError> {
        PackageProperties::parse(resource, version, &body, etag)
    }
}

/// The ABAP program object type.
#[derive(Debug)]
pub enum Program {}

impl ObjectType for Program {
    const WORKBENCH_TYPE: GlobalWorkbenchType = GlobalWorkbenchType::new("PROG", "P");
    const NAMING_POLICY: ObjectNamePolicy = ObjectNamePolicy::new(30);
}

impl ObjectCollection for Program {
    const CATEGORY: CategoryId = PROGRAMS;
}

impl Source for Program {}

impl ObjectProperties for Program {
    type MediaVersion = ProgramPropertiesVersion;
    type Properties = ProgramProperties;

    fn parse(
        resource: &ObjectRef<Self>,
        version: Self::MediaVersion,
        body: Vec<u8>,
        etag: Option<EntityTag>,
    ) -> Result<Self::Properties, ResponseError> {
        ProgramProperties::parse(resource, version, &body, etag)
    }
}

/// The standalone ABAP include object type.
#[derive(Debug)]
pub enum Include {}

impl ObjectType for Include {
    const WORKBENCH_TYPE: GlobalWorkbenchType = GlobalWorkbenchType::new("PROG", "I");
    const NAMING_POLICY: ObjectNamePolicy = ObjectNamePolicy::new(40);
}

impl ObjectCollection for Include {
    const CATEGORY: CategoryId = INCLUDES;
}

impl Source for Include {}

impl ObjectProperties for Include {
    type MediaVersion = IncludePropertyVersion;
    type Properties = IncludeProperties;

    fn parse(
        resource: &ObjectRef<Self>,
        version: Self::MediaVersion,
        body: Vec<u8>,
        etag: Option<EntityTag>,
    ) -> Result<Self::Properties, ResponseError> {
        IncludeProperties::parse(resource, version, &body, etag)
    }
}

/// An ABAP class object.
#[derive(Debug)]
pub enum Class {}

impl ObjectType for Class {
    const WORKBENCH_TYPE: GlobalWorkbenchType = GlobalWorkbenchType::new("CLAS", "OC");
    const NAMING_POLICY: ObjectNamePolicy = ObjectNamePolicy::new(30);
}

impl ObjectCollection for Class {
    const CATEGORY: CategoryId = CLASSES;
}

impl Source for Class {}

/// A source component owned and locked by an ABAP class.
///
/// Local class includes are ADT resources beneath the class object rather than
/// independent repository objects.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ClassSourceComponent {
    Main,
    Definitions,
    Implementations,
    Macros,
    TestClasses,
}

impl ClassSourceComponent {
    /// Returns the component name used by ADT.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Definitions => "definitions",
            Self::Implementations => "implementations",
            Self::Macros => "macros",
            Self::TestClasses => "testclasses",
        }
    }

    const fn path(self) -> &'static [&'static str] {
        match self {
            Self::Main => &["source", "main"],
            Self::Definitions => &["includes", "definitions"],
            Self::Implementations => &["includes", "implementations"],
            Self::Macros => &["includes", "macros"],
            Self::TestClasses => &["includes", "testclasses"],
        }
    }
}

impl ObjectRef<Class> {
    /// Resolves one of the source resources owned by this class.
    pub fn component_source(&self, component: ClassSourceComponent) -> SourceRef {
        let uri = self
            .uri()
            .append_segments(component.path())
            .expect("static class source path segments form a valid ADT URI");
        SourceRef::new(self.erase(), uri)
    }
}

impl ObjectRef<Program> {
    #[cfg(test)]
    pub(crate) fn for_test(name: &str, uri: crate::AdtUri) -> Self {
        Self::typed(name.to_ascii_uppercase(), uri)
    }
}

impl ObjectRef<Package> {
    #[cfg(test)]
    pub(crate) fn for_test(name: &str, uri: crate::AdtUri) -> Self {
        Self::typed(name.to_ascii_uppercase(), uri)
    }
}

impl ObjectRef<Include> {
    #[cfg(test)]
    pub(crate) fn for_test(name: &str, uri: crate::AdtUri) -> Self {
        Self::typed(name.to_ascii_uppercase(), uri)
    }
}

impl ObjectRef<Class> {
    #[cfg(test)]
    pub(crate) fn for_test(name: &str, uri: crate::AdtUri) -> Self {
        Self::typed(name.to_ascii_uppercase(), uri)
    }
}
