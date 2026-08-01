use serde::Deserialize;

use crate::{
    AdtUri, EntityTag, GlobalWorkbenchType, NegotiableMediaVersion, ObjectError, ObjectRef,
    ObjectType, ObjectVersion, Package, ResponseError,
    resource::{AdvertisedLink, Relations, resolve_href},
};

const PACKAGE_TYPE_KEY: &str = "DEVCK";
const PACKAGE_INTERFACE_TYPE: &str = "PINF/KI";
const PACKAGE_INTERFACE_TYPE_KEY: &str = "PINFKI";

/// The SAP media-type version used to decode package properties.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PackagePropertiesVersion {
    /// Package properties V1.
    V1,

    /// Package properties V2.
    V2,
}

impl NegotiableMediaVersion for PackagePropertiesVersion {
    const SUPPORTED: &'static [Self] = &[Self::V2, Self::V1];

    fn media_type(self) -> &'static str {
        match self {
            Self::V1 => "application/vnd.sap.adt.packages.v1+xml",
            Self::V2 => "application/vnd.sap.adt.packages.v2+xml",
        }
    }
}

/// Package properties tagged with the media-type version returned by ADT.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum PackageProperties {
    /// A V1 package-properties response.
    V1(Box<PackagePropertiesV1>),

    /// A V2 package-properties response.
    V2(Box<PackagePropertiesV2>),
}

impl PackageProperties {
    /// Returns the response media-type version.
    pub fn media_version(&self) -> PackagePropertiesVersion {
        match self {
            Self::V1(_) => PackagePropertiesVersion::V1,
            Self::V2(_) => PackagePropertiesVersion::V2,
        }
    }

    /// Returns the response entity tag, when present.
    pub fn etag(&self) -> Option<&EntityTag> {
        match self {
            Self::V1(package) | Self::V2(package) => package.etag.as_ref(),
        }
    }

    pub(crate) fn parse(
        resource: &ObjectRef<Package>,
        media_version: PackagePropertiesVersion,
        body: &[u8],
        etag: Option<EntityTag>,
    ) -> Result<Self, ResponseError> {
        let raw: RawPackageProperties =
            serde_xml_rs::from_reader(body).map_err(ObjectError::InvalidResponse)?;
        let properties = PackagePropertiesV2::from_raw(resource.clone(), raw, etag)?;
        Ok(match media_version {
            PackagePropertiesVersion::V1 => Self::V1(Box::new(properties)),
            PackagePropertiesVersion::V2 => Self::V2(Box::new(properties)),
        })
    }
}

/// The V1 package-properties representation uses the V2 payload schema.
pub type PackagePropertiesV1 = PackagePropertiesV2;

/// Properties of an ABAP package.
#[derive(Clone, Debug)]
pub struct PackagePropertiesV2 {
    /// The package resource that was fetched.
    pub reference: ObjectRef<Package>,
    /// The package name supplied by SAP.
    pub name: String,
    /// The repository object type, normally `DEVC/K`.
    pub object_type: GlobalWorkbenchType,
    /// The timestamp at which the package was last changed.
    pub last_changed: String,
    /// The active or inactive object version.
    pub version: ObjectVersion,
    /// The timestamp at which the package was created.
    pub created_at: String,
    /// The user who last changed the package.
    pub changed_by: String,
    /// The user who created the package.
    pub created_by: String,
    /// The package description.
    pub description: String,
    /// The maximum package-description length.
    pub description_text_limit: u32,
    /// The package's logon language.
    pub language: String,
    /// The user responsible for the package.
    pub responsible: String,
    /// The package's master language.
    pub master_language: String,
    /// The package's master system, when advertised.
    pub master_system: Option<String>,
    /// Package behavior and editor capability flags.
    pub attributes: PackageAttributes,
    /// The parent package, when this is not a root package.
    pub super_package: Option<PackageReference>,
    /// The assigned application component.
    pub application_component: PackageAssignment,
    /// Software-component and transport-layer assignments.
    pub transport: PackageTransport,
    /// Whether use accesses are shown by the package editor.
    pub use_accesses_visible: bool,
    /// Package-interface use accesses.
    pub use_accesses: Vec<PackageUseAccess>,
    /// Whether package interfaces are shown by the package editor.
    pub package_interfaces_visible: bool,
    /// Interfaces defined by this package.
    pub package_interfaces: Vec<PackageInterfaceReference>,
    /// Direct subpackages included in the properties representation.
    pub sub_packages: Vec<PackageReference>,
    /// The entity tag of these properties, when present.
    pub etag: Option<EntityTag>,
    relations: Relations,
}

impl PackagePropertiesV2 {
    /// Returns the package's advertised links without resolving them eagerly.
    pub fn relations(&self) -> &Relations {
        &self.relations
    }

    fn from_raw(
        reference: ObjectRef<Package>,
        raw: RawPackageProperties,
        etag: Option<EntityTag>,
    ) -> Result<Self, ObjectError> {
        if raw.object_type != Package::WORKBENCH_TYPE {
            return Err(ObjectError::UnexpectedObjectType {
                expected: Package::WORKBENCH_TYPE,
                actual: raw.object_type,
            });
        }
        let version = ObjectVersion::parse(&raw.version).ok_or_else(|| {
            ObjectError::UnsupportedObjectVersion {
                version: raw.version.clone(),
            }
        })?;
        let base = reference.uri();
        let super_package = package_reference(raw.super_package, base, false)?;
        let use_accesses = raw
            .use_accesses
            .items
            .into_iter()
            .map(|access| {
                Ok(PackageUseAccess {
                    severity: access.severity,
                    package_interface: package_interface_reference(access.package_interface, base)?,
                    package: package_reference(access.package, base, false)?,
                })
            })
            .collect::<Result<Vec<_>, ObjectError>>()?;
        let package_interfaces = raw
            .package_interfaces
            .items
            .into_iter()
            .map(|item| package_interface_reference(item, base))
            .collect::<Result<Vec<_>, _>>()?;
        let sub_packages = raw
            .sub_packages
            .items
            .into_iter()
            .map(|item| package_reference(item, base, false))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        let relations = Relations::new(reference.erase(), raw.links);

        Ok(Self {
            reference,
            name: raw.name,
            object_type: raw.object_type,
            last_changed: raw.last_changed,
            version,
            created_at: raw.created_at,
            changed_by: raw.changed_by,
            created_by: raw.created_by,
            description: raw.description,
            description_text_limit: raw.description_text_limit,
            language: raw.language,
            responsible: raw.responsible,
            master_language: raw.master_language,
            master_system: raw.master_system,
            attributes: raw.attributes.into(),
            super_package,
            application_component: raw.application_component.into(),
            transport: raw.transport.into(),
            use_accesses_visible: raw.use_accesses.visible,
            use_accesses,
            package_interfaces_visible: raw.package_interfaces.visible,
            package_interfaces,
            sub_packages,
            etag,
            relations,
        })
    }
}

/// Package behavior and editor capability flags.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageAttributes {
    /// The semantic package type, such as `development`.
    pub package_type: String,
    /// Whether the package type is editable.
    pub package_type_editable: bool,
    /// Whether repository objects can be assigned to the package.
    pub adding_objects_allowed: bool,
    /// Whether object-assignment behavior is editable.
    pub adding_objects_allowed_editable: bool,
    /// Whether package encapsulation is enabled.
    pub encapsulated: bool,
    /// Whether encapsulation is editable.
    pub encapsulation_editable: bool,
    /// Whether encapsulation is shown by the package editor.
    pub encapsulation_visible: bool,
    /// Whether changes assigned to the package are recorded for transport.
    pub record_changes: bool,
    /// Whether change recording is editable.
    pub record_changes_editable: bool,
    /// Whether switch assignment is shown by the package editor.
    pub switch_visible: bool,
    /// The configured ABAP language version.
    pub language_version: String,
    /// Whether the language version is shown by the package editor.
    pub language_version_visible: bool,
    /// Whether the language version is editable.
    pub language_version_editable: bool,
}

/// A named package assignment with editor visibility and mutability flags.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageAssignment {
    /// The assigned value.
    pub name: String,
    /// The server-provided value description.
    pub description: String,
    /// Whether this assignment is shown by the package editor.
    pub visible: bool,
    /// Whether this assignment is editable.
    pub editable: bool,
}

/// Software-component and transport-layer assignments.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageTransport {
    /// The package's software component.
    pub software_component: PackageAssignment,
    /// The package's transport layer.
    pub transport_layer: PackageAssignment,
}

/// A typed package reference and its optional short description.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageReference {
    /// The typed package resource.
    pub reference: ObjectRef<Package>,
    /// The package short description, when advertised.
    pub description: Option<String>,
}

/// A package-interface reference advertised through a package representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageInterfaceReference {
    /// The package-interface name.
    pub name: String,
    /// The validated package-interface resource URI.
    pub uri: AdtUri,
    /// The wire object type, either `PINF/KI` or compact `PINFKI`.
    pub object_type: String,
    /// The package-interface description, when advertised.
    pub description: Option<String>,
}

/// A package-interface use access and the package that owns it, when advertised.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageUseAccess {
    /// The backend-defined use-access severity.
    pub severity: String,
    /// The package interface being consumed.
    pub package_interface: PackageInterfaceReference,
    /// The package that owns the interface, when advertised.
    pub package: Option<PackageReference>,
}

/// Which side of a package hierarchy to request.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PackageTreeKind {
    /// Fetch the package and its ancestors.
    Super,
    /// Fetch the package's immediate subpackages.
    Sub,
}

impl PackageTreeKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Super => "super",
            Self::Sub => "sub",
        }
    }
}

/// A package hierarchy returned by the package tree resource.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageTree {
    /// Whether this response represents an ancestor tree.
    pub is_super_tree: bool,
    /// Package nodes in backend response order.
    pub nodes: Vec<PackageTreeNode>,
}

impl PackageTree {
    pub(crate) fn parse(body: &[u8], base: &AdtUri) -> Result<Self, ResponseError> {
        let raw: RawPackageTree =
            serde_xml_rs::from_reader(body).map_err(ObjectError::InvalidResponse)?;
        let nodes = raw
            .nodes
            .into_iter()
            .map(|node| PackageTreeNode::from_raw(node, base))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            is_super_tree: raw.is_super_tree,
            nodes,
        })
    }
}

/// One package and its interfaces in a package hierarchy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageTreeNode {
    /// The package represented by this node.
    pub package: PackageReference,
    /// Whether the package is encapsulated.
    pub encapsulated: bool,
    /// Whether the package has direct subpackages.
    pub has_subpackages: bool,
    /// Whether the package defines package interfaces.
    pub has_interfaces: bool,
    /// The direct parent package, when advertised.
    pub super_package: Option<PackageReference>,
    /// Interfaces defined by this package.
    pub package_interfaces: Vec<PackageInterfaceReference>,
}

impl PackageTreeNode {
    fn from_raw(raw: RawPackageTreeNode, base: &AdtUri) -> Result<Self, ObjectError> {
        let package = package_reference(
            RawObjectReference {
                uri: Some(raw.uri),
                object_type: Some(raw.object_type),
                name: Some(raw.name),
                description: raw.description,
            },
            base,
            true,
        )?
        .ok_or(ObjectError::IncompleteObjectReference {
            field: "adtcore:name",
        })?;
        let super_package = package_reference(raw.super_package, base, true)?;
        let package_interfaces = raw
            .package_interfaces
            .items
            .into_iter()
            .map(|item| package_interface_reference(item, base))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            package,
            encapsulated: raw.encapsulated,
            has_subpackages: raw.has_subpackages,
            has_interfaces: raw.has_interfaces,
            super_package,
            package_interfaces,
        })
    }
}

/// Global package editor settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackageSettings {
    /// Whether package-check errors should be shown in the package editor.
    pub show_package_check_errors: bool,
}

impl PackageSettings {
    pub(crate) fn parse(body: &[u8]) -> Result<Self, ResponseError> {
        let raw: RawPackageSettings =
            serde_xml_rs::from_reader(body).map_err(ObjectError::InvalidResponse)?;
        Ok(Self {
            show_package_check_errors: raw.show_package_check_errors,
        })
    }
}

fn package_reference(
    raw: RawObjectReference,
    base: &AdtUri,
    compact_type: bool,
) -> Result<Option<PackageReference>, ObjectError> {
    if raw.name.is_none() && raw.uri.is_none() && raw.object_type.is_none() {
        return Ok(None);
    }
    let name = required(raw.name, "adtcore:name")?;
    let href = required(raw.uri, "adtcore:uri")?;
    let object_type = required(raw.object_type, "adtcore:type")?;
    if compact_type {
        if object_type != PACKAGE_TYPE_KEY {
            return Err(ObjectError::UnexpectedCompactObjectType {
                expected: PACKAGE_TYPE_KEY,
                actual: object_type,
            });
        }
    } else {
        let actual: GlobalWorkbenchType =
            object_type
                .parse()
                .map_err(|_| ObjectError::UnexpectedCompactObjectType {
                    expected: "DEVC/K",
                    actual: object_type.clone(),
                })?;
        if actual != Package::WORKBENCH_TYPE {
            return Err(ObjectError::UnexpectedObjectType {
                expected: Package::WORKBENCH_TYPE,
                actual,
            });
        }
    }
    let target = resolve_href(base, &href)
        .map_err(|source| ObjectError::InvalidLink {
            href: href.clone(),
            source,
        })?
        .target;
    let reference = ObjectRef::<Package>::from_parts(name, target)?;
    Ok(Some(PackageReference {
        reference,
        description: raw.description,
    }))
}

fn package_interface_reference(
    raw: RawObjectReference,
    base: &AdtUri,
) -> Result<PackageInterfaceReference, ObjectError> {
    let name = required(raw.name, "adtcore:name")?;
    let href = required(raw.uri, "adtcore:uri")?;
    let object_type = required(raw.object_type, "adtcore:type")?;
    if object_type != PACKAGE_INTERFACE_TYPE && object_type != PACKAGE_INTERFACE_TYPE_KEY {
        return Err(ObjectError::UnexpectedCompactObjectType {
            expected: PACKAGE_INTERFACE_TYPE,
            actual: object_type,
        });
    }
    let uri = resolve_href(base, &href)
        .map_err(|source| ObjectError::InvalidLink {
            href: href.clone(),
            source,
        })?
        .target;
    Ok(PackageInterfaceReference {
        name,
        uri,
        object_type,
        description: raw.description,
    })
}

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ObjectError> {
    value.ok_or(ObjectError::IncompleteObjectReference { field })
}

#[derive(Deserialize)]
#[serde(rename = "pak:package")]
struct RawPackageProperties {
    #[serde(rename = "@adtcore:name")]
    name: String,
    #[serde(rename = "@adtcore:type")]
    object_type: GlobalWorkbenchType,
    #[serde(rename = "@adtcore:changedAt")]
    last_changed: String,
    #[serde(rename = "@adtcore:version")]
    version: String,
    #[serde(rename = "@adtcore:createdAt")]
    created_at: String,
    #[serde(rename = "@adtcore:changedBy")]
    changed_by: String,
    #[serde(rename = "@adtcore:createdBy")]
    created_by: String,
    #[serde(rename = "@adtcore:description")]
    description: String,
    #[serde(rename = "@adtcore:descriptionTextLimit")]
    description_text_limit: u32,
    #[serde(rename = "@adtcore:language")]
    language: String,
    #[serde(rename = "@adtcore:responsible")]
    responsible: String,
    #[serde(rename = "@adtcore:masterLanguage")]
    master_language: String,
    #[serde(rename = "@adtcore:masterSystem")]
    master_system: Option<String>,
    #[serde(rename = "atom:link", default)]
    links: Vec<AdvertisedLink>,
    #[serde(rename = "pak:attributes")]
    attributes: RawPackageAttributes,
    #[serde(rename = "pak:superPackage", default)]
    super_package: RawObjectReference,
    #[serde(rename = "pak:applicationComponent")]
    application_component: RawPackageAssignment,
    #[serde(rename = "pak:transport")]
    transport: RawPackageTransport,
    #[serde(rename = "pak:useAccesses", default)]
    use_accesses: RawUseAccesses,
    #[serde(rename = "pak:packageInterfaces", default)]
    package_interfaces: RawPackageInterfaces,
    #[serde(rename = "pak:subPackages", default)]
    sub_packages: RawSubPackages,
}

#[derive(Deserialize)]
struct RawPackageAttributes {
    #[serde(rename = "@pak:packageType")]
    package_type: String,
    #[serde(rename = "@pak:isPackageTypeEditable")]
    package_type_editable: bool,
    #[serde(rename = "@pak:isAddingObjectsAllowed")]
    adding_objects_allowed: bool,
    #[serde(rename = "@pak:isAddingObjectsAllowedEditable")]
    adding_objects_allowed_editable: bool,
    #[serde(rename = "@pak:isEncapsulated")]
    encapsulated: bool,
    #[serde(rename = "@pak:isEncapsulationEditable")]
    encapsulation_editable: bool,
    #[serde(rename = "@pak:isEncapsulationVisible")]
    encapsulation_visible: bool,
    #[serde(rename = "@pak:recordChanges")]
    record_changes: bool,
    #[serde(rename = "@pak:isRecordChangesEditable")]
    record_changes_editable: bool,
    #[serde(rename = "@pak:isSwitchVisible")]
    switch_visible: bool,
    #[serde(rename = "@pak:languageVersion", default)]
    language_version: String,
    #[serde(rename = "@pak:isLanguageVersionVisible")]
    language_version_visible: bool,
    #[serde(rename = "@pak:isLanguageVersionEditable")]
    language_version_editable: bool,
}

impl From<RawPackageAttributes> for PackageAttributes {
    fn from(raw: RawPackageAttributes) -> Self {
        Self {
            package_type: raw.package_type,
            package_type_editable: raw.package_type_editable,
            adding_objects_allowed: raw.adding_objects_allowed,
            adding_objects_allowed_editable: raw.adding_objects_allowed_editable,
            encapsulated: raw.encapsulated,
            encapsulation_editable: raw.encapsulation_editable,
            encapsulation_visible: raw.encapsulation_visible,
            record_changes: raw.record_changes,
            record_changes_editable: raw.record_changes_editable,
            switch_visible: raw.switch_visible,
            language_version: raw.language_version,
            language_version_visible: raw.language_version_visible,
            language_version_editable: raw.language_version_editable,
        }
    }
}

#[derive(Deserialize)]
struct RawPackageAssignment {
    #[serde(rename = "@pak:name", default)]
    name: String,
    #[serde(rename = "@pak:description", default)]
    description: String,
    #[serde(rename = "@pak:isVisible")]
    visible: bool,
    #[serde(rename = "@pak:isEditable")]
    editable: bool,
}

impl From<RawPackageAssignment> for PackageAssignment {
    fn from(raw: RawPackageAssignment) -> Self {
        Self {
            name: raw.name,
            description: raw.description,
            visible: raw.visible,
            editable: raw.editable,
        }
    }
}

#[derive(Deserialize)]
struct RawPackageTransport {
    #[serde(rename = "pak:softwareComponent")]
    software_component: RawPackageAssignment,
    #[serde(rename = "pak:transportLayer")]
    transport_layer: RawPackageAssignment,
}

impl From<RawPackageTransport> for PackageTransport {
    fn from(raw: RawPackageTransport) -> Self {
        Self {
            software_component: raw.software_component.into(),
            transport_layer: raw.transport_layer.into(),
        }
    }
}

#[derive(Default, Deserialize)]
struct RawUseAccesses {
    #[serde(rename = "@pak:isVisible", default)]
    visible: bool,
    #[serde(rename = "pak:useAccess", default)]
    items: Vec<RawUseAccess>,
}

#[derive(Deserialize)]
struct RawUseAccess {
    #[serde(rename = "@pak:severity")]
    severity: String,
    #[serde(rename = "pak:packageInterfaceRef")]
    package_interface: RawObjectReference,
    #[serde(rename = "pak:packageRef", default)]
    package: RawObjectReference,
}

#[derive(Default, Deserialize)]
struct RawPackageInterfaces {
    #[serde(rename = "@pak:isVisible", default)]
    visible: bool,
    #[serde(rename = "pak:packageInterfaceRef", default)]
    items: Vec<RawObjectReference>,
}

#[derive(Default, Deserialize)]
struct RawSubPackages {
    #[serde(rename = "pak:packageRef", default)]
    items: Vec<RawObjectReference>,
}

#[derive(Clone, Default, Deserialize)]
struct RawObjectReference {
    #[serde(rename = "@adtcore:uri")]
    uri: Option<String>,
    #[serde(rename = "@adtcore:type")]
    object_type: Option<String>,
    #[serde(rename = "@adtcore:name")]
    name: Option<String>,
    #[serde(rename = "@adtcore:description")]
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename = "pak:packageTree")]
struct RawPackageTree {
    #[serde(rename = "@pak:isSuperTree")]
    is_super_tree: bool,
    #[serde(rename = "pak:treeNode", default)]
    nodes: Vec<RawPackageTreeNode>,
}

#[derive(Deserialize)]
struct RawPackageTreeNode {
    #[serde(rename = "@adtcore:uri")]
    uri: String,
    #[serde(rename = "@adtcore:type")]
    object_type: String,
    #[serde(rename = "@adtcore:name")]
    name: String,
    #[serde(rename = "@adtcore:description")]
    description: Option<String>,
    #[serde(rename = "@pak:isEncapsulated")]
    encapsulated: bool,
    #[serde(rename = "@pak:hasSubpackages")]
    has_subpackages: bool,
    #[serde(rename = "@pak:hasInterfaces")]
    has_interfaces: bool,
    #[serde(rename = "pak:superPackageRef", default)]
    super_package: RawObjectReference,
    #[serde(rename = "pak:packageInterfaces", default)]
    package_interfaces: RawPackageInterfaces,
}

#[derive(Deserialize)]
#[serde(rename = "pkcs:settings")]
struct RawPackageSettings {
    #[serde(rename = "@pkcs:showPackageCheckErrors")]
    show_package_check_errors: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKAGE_XML: &[u8] = include_bytes!("../../tests/fixtures/package-sadt-tools-core.xml");
    const SUPER_TREE_XML: &[u8] = include_bytes!("../../tests/fixtures/package-tree-super.xml");
    const SETTINGS_XML: &[u8] = include_bytes!("../../tests/fixtures/package-settings.xml");

    fn package_reference() -> ObjectRef<Package> {
        ObjectRef::<Package>::for_test(
            "SADT_TOOLS_CORE",
            AdtUri::parse("/sap/bc/adt/packages/sadt_tools_core").unwrap(),
        )
    }

    #[test]
    fn parses_live_package_properties() {
        let properties = PackageProperties::parse(
            &package_reference(),
            PackagePropertiesVersion::V2,
            PACKAGE_XML,
            Some(EntityTag::try_from("package-etag").unwrap()),
        )
        .unwrap();
        let PackageProperties::V2(properties) = properties else {
            panic!("unexpected package-properties version");
        };

        assert_eq!(properties.name, "SADT_TOOLS_CORE");
        assert_eq!(properties.object_type, Package::WORKBENCH_TYPE);
        assert_eq!(properties.version, ObjectVersion::Active);
        assert_eq!(properties.master_system.as_deref(), Some("SAP"));
        assert!(properties.attributes.encapsulated);
        assert!(properties.attributes.record_changes);
        assert_eq!(
            properties.super_package.as_ref().unwrap().reference.name(),
            "SADT_MAIN"
        );
        assert_eq!(properties.application_component.name, "BC-DWB-AIE");
        assert_eq!(properties.transport.software_component.name, "SAP_BASIS");
        assert_eq!(properties.use_accesses.len(), 1);
        assert_eq!(
            properties.use_accesses[0]
                .package
                .as_ref()
                .unwrap()
                .reference
                .name(),
            "SADT_CORE"
        );
        assert_eq!(properties.package_interfaces.len(), 1);
        assert_eq!(
            properties.sub_packages[0].reference.name(),
            "SADT_TOOLS_CORE_TEST"
        );
        assert_eq!(properties.etag.as_deref(), Some("package-etag"));
        assert_eq!(properties.relations().len(), 1);
    }

    #[test]
    fn parses_live_package_tree() {
        let base = AdtUri::parse("/sap/bc/adt/packages/$tree").unwrap();
        let tree = PackageTree::parse(SUPER_TREE_XML, &base).unwrap();

        assert!(tree.is_super_tree);
        assert_eq!(tree.nodes.len(), 2);
        assert_eq!(tree.nodes[0].package.reference.name(), "SADT_TOOLS_CORE");
        assert_eq!(
            tree.nodes[0]
                .super_package
                .as_ref()
                .unwrap()
                .reference
                .name(),
            "SADT_MAIN"
        );
        assert_eq!(tree.nodes[0].package_interfaces.len(), 1);
        assert!(tree.nodes[1].has_subpackages);
    }

    #[test]
    fn rejects_an_unexpected_compact_tree_type() {
        let xml = String::from_utf8(SUPER_TREE_XML.to_vec())
            .unwrap()
            .replacen("adtcore:type=\"DEVCK\"", "adtcore:type=\"PROGP\"", 1);

        let base = AdtUri::parse("/sap/bc/adt/packages/$tree").unwrap();
        let error = PackageTree::parse(xml.as_bytes(), &base).unwrap_err();

        assert!(matches!(
            error,
            ResponseError::Object(ObjectError::UnexpectedCompactObjectType {
                expected: PACKAGE_TYPE_KEY,
                actual,
            }) if actual == "PROGP"
        ));
    }

    #[test]
    fn parses_package_settings() {
        assert_eq!(
            PackageSettings::parse(SETTINGS_XML).unwrap(),
            PackageSettings {
                show_package_check_errors: false
            }
        );
    }
}
