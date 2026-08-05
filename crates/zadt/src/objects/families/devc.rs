use super::super::{
    GlobalWorkbenchType, ObjectCollection, ObjectNamePolicy, ObjectProperties, ObjectRef,
    ObjectType, private,
};
use crate::{
    error::ResponseError,
    models::{PackageProperties, PackagePropertiesVersion},
    protocol::EntityTag,
    vocabulary::CategoryId,
};

/// The package (devclass) object type.
#[derive(Debug)]
pub enum Package {}

impl private::Sealed for Package {}

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

impl ObjectRef<Package> {
    #[cfg(test)]
    pub(crate) fn for_test(name: &str, uri: crate::AdtUri) -> Self {
        Self::typed(name.to_ascii_uppercase(), uri)
    }
}
