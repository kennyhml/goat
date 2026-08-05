use super::super::{
    GlobalWorkbenchType, MainSource, ObjectCollection, ObjectNamePolicy, ObjectProperties,
    ObjectRef, ObjectType, Source, SourceComponent, private,
};
use crate::{
    error::ResponseError,
    models::{
        IncludeProperties, IncludePropertyVersion, ProgramProperties, ProgramPropertiesVersion,
    },
    protocol::EntityTag,
    vocabulary::CategoryId,
};

/// The ABAP program object type.
#[derive(Debug)]
pub enum Program {}

impl private::Sealed for Program {}

impl ObjectType for Program {
    const WORKBENCH_TYPE: GlobalWorkbenchType = GlobalWorkbenchType::new("PROG", "P");
    const NAMING_POLICY: ObjectNamePolicy = ObjectNamePolicy::new(30);
    const SOURCE_COMPONENTS: &'static [&'static dyn SourceComponent] = &[&MainSource];
}

impl ObjectCollection for Program {
    const CATEGORY: CategoryId = CategoryId {
        scheme: "http://www.sap.com/adt/categories/programs",
        term: "programs",
    };
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

impl private::Sealed for Include {}

impl ObjectType for Include {
    const WORKBENCH_TYPE: GlobalWorkbenchType = GlobalWorkbenchType::new("PROG", "I");
    const NAMING_POLICY: ObjectNamePolicy = ObjectNamePolicy::new(40);
    const SOURCE_COMPONENTS: &'static [&'static dyn SourceComponent] = &[&MainSource];
}

impl ObjectCollection for Include {
    const CATEGORY: CategoryId = CategoryId {
        scheme: "http://www.sap.com/adt/categories/programs",
        term: "includes",
    };
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
