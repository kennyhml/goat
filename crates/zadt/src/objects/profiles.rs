use super::{
    GlobalWorkbenchType, Lock, ObjectNamePolicy, ObjectProperties, ObjectRef, ObjectType, Source,
    private,
};
use crate::{
    error::ResponseError,
    models::{
        IncludeProperties, IncludePropertyVersion, ProgramProperties, ProgramPropertiesVersion,
    },
    protocol::EntityTag,
    vocabulary::{CategoryId, INCLUDES, PROGRAMS},
};

impl private::Sealed for Package {}
impl private::Sealed for Program {}
impl private::Sealed for Include {}

/// The package (devclass) object type.
#[derive(Debug)]
pub enum Package {}

impl ObjectType for Package {
    const CATEGORY: CategoryId = CategoryId {
        scheme: "http://www.sap.com/wbobj/packages",
        term: "devck",
    };
    const TYPE: GlobalWorkbenchType = GlobalWorkbenchType::new("DEVC", "K");
    const NAME_POLICY: ObjectNamePolicy = ObjectNamePolicy::new(30);
}

impl Lock for Package {}
// TODO: package object properties

/// The ABAP program object type.
#[derive(Debug)]
pub enum Program {}

impl ObjectType for Program {
    const CATEGORY: CategoryId = PROGRAMS;
    const TYPE: GlobalWorkbenchType = GlobalWorkbenchType::new("PROG", "P");
    const NAME_POLICY: ObjectNamePolicy = ObjectNamePolicy::new(30);
}

impl Lock for Program {}
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
    const CATEGORY: CategoryId = INCLUDES;
    const TYPE: GlobalWorkbenchType = GlobalWorkbenchType::new("PROG", "I");
    const NAME_POLICY: ObjectNamePolicy = ObjectNamePolicy::new(40);
}

impl Lock for Include {}
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

impl ObjectRef<Program> {
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
