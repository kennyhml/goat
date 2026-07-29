use super::{
    GlobalWorkbenchType, Lock, ObjectNamePolicy, ObjectProperties, ObjectRef, ObjectType, private,
};
use crate::{
    error::ResponseError,
    models::{
        IncludeMediaVersion, IncludeProperties, ProgramMediaVersion, ProgramProperties,
        parse_include_properties, parse_program_properties,
    },
    protocol::EntityTag,
    vocabulary::{CategoryId, INCLUDES, PROGRAMS},
};

/// The ABAP program object type.
#[derive(Debug)]
pub enum Program {}

impl ObjectType for Program {
    const CATEGORY: CategoryId = PROGRAMS;
    const TYPE: GlobalWorkbenchType = GlobalWorkbenchType::new("PROG", "P");
    const NAME_POLICY: ObjectNamePolicy = ObjectNamePolicy::new(30);
}

impl Lock for Program {}

impl ObjectProperties for Program {
    type MediaVersion = ProgramMediaVersion;
    type Properties = ProgramProperties;

    fn parse(
        resource: &ObjectRef<Self>,
        version: Self::MediaVersion,
        body: Vec<u8>,
        etag: Option<EntityTag>,
    ) -> Result<Self::Properties, ResponseError> {
        parse_program_properties(resource, version, &body, etag).map_err(Into::into)
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

impl ObjectProperties for Include {
    type MediaVersion = IncludeMediaVersion;
    type Properties = IncludeProperties;

    fn parse(
        resource: &ObjectRef<Self>,
        version: Self::MediaVersion,
        body: Vec<u8>,
        etag: Option<EntityTag>,
    ) -> Result<Self::Properties, ResponseError> {
        parse_include_properties(resource, version, &body, etag).map_err(Into::into)
    }
}

impl private::Sealed for Program {}
impl private::Sealed for Include {}

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
