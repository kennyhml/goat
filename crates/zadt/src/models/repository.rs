use std::{borrow::Cow, fmt};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    AdtUri, GlobalWorkbenchType, InvalidWorkbenchType, ObjectError, ObjectRef, Package,
    RepositoryError,
    resource::{AdvertisedLink, Relations},
};

const VIRTUAL_FOLDERS_NAMESPACE: &str = "http://www.sap.com/adt/ris/virtualFolders";
const PACKAGE_RELATION: &str = "http://www.sap.com/adt/relations/packages";

/// A repository information system facet key.
///
/// SAP defines a common set of keys, exposed as associated constants, but
/// systems may advertise additional facets. Unknown keys are therefore kept
/// intact instead of being rejected by a closed enum.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RepositoryFacet(Cow<'static, str>);

impl RepositoryFacet {
    pub const PACKAGE: Self = Self(Cow::Borrowed("PACKAGE"));
    pub const GROUP: Self = Self(Cow::Borrowed("GROUP"));
    pub const TYPE: Self = Self(Cow::Borrowed("TYPE"));
    pub const OWNER: Self = Self(Cow::Borrowed("OWNER"));
    pub const API_STATE: Self = Self(Cow::Borrowed("API"));
    pub const APPLICATION_COMPONENT: Self = Self(Cow::Borrowed("APPL"));
    pub const FAVORITES: Self = Self(Cow::Borrowed("FAV"));
    pub const CREATED: Self = Self(Cow::Borrowed("CREATED"));
    pub const CREATION_MONTH: Self = Self(Cow::Borrowed("MONTH"));
    pub const CREATION_DATE: Self = Self(Cow::Borrowed("DATE"));
    pub const LANGUAGE: Self = Self(Cow::Borrowed("LANGUAGE"));
    pub const SOURCE_SYSTEM: Self = Self(Cow::Borrowed("SYSTEM"));
    pub const VERSION: Self = Self(Cow::Borrowed("VERSION"));
    pub const DOCUMENTATION: Self = Self(Cow::Borrowed("DOCU"));

    /// Returns the exact facet key used by RIS.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for RepositoryFacet {
    fn from(value: &str) -> Self {
        Self(Cow::Owned(value.to_owned()))
    }
}

impl From<String> for RepositoryFacet {
    fn from(value: String) -> Self {
        Self(Cow::Owned(value))
    }
}

impl fmt::Display for RepositoryFacet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for RepositoryFacet {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RepositoryFacet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::from)
    }
}

/// An object type emitted by the repository information system.
///
/// RIS may return either a global Workbench type such as `CLAS/OC` or a
/// four-character alias such as `AUTH`. The wire value is therefore preserved
/// instead of being forced into [`GlobalWorkbenchType`].
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RepositoryObjectType(String);

impl RepositoryObjectType {
    /// Returns the exact object type emitted by RIS.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Parses this value as a global Workbench type when it has that form.
    pub fn global_workbench_type(&self) -> Result<GlobalWorkbenchType, InvalidWorkbenchType> {
        self.0.parse()
    }
}

impl From<&str> for RepositoryObjectType {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for RepositoryObjectType {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl fmt::Display for RepositoryObjectType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RepositoryObjectType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::from)
    }
}

/// A filter applied before RIS structures or returns repository objects.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename = "vfs:preselection")]
pub struct RepositoryPreselection {
    #[serde(rename = "@facet")]
    facet: RepositoryFacet,

    #[serde(rename = "vfs:value")]
    values: Vec<String>,
}

impl RepositoryPreselection {
    /// Creates an inclusive filter containing one value.
    pub fn new(facet: impl Into<RepositoryFacet>, value: impl Into<String>) -> Self {
        Self {
            facet: facet.into(),
            values: vec![value.into()],
        }
    }

    /// Selects objects assigned directly to a package, excluding subpackages.
    ///
    /// The leading `..` is RIS protocol syntax and does not denote filesystem
    /// parent traversal.
    pub fn direct_package(package: impl Into<String>) -> Self {
        let package = package.into();
        let package = package.strip_prefix("..").unwrap_or(&package);
        Self::new(RepositoryFacet::PACKAGE, format!("..{package}"))
    }

    /// Adds another included value.
    pub fn include(mut self, value: impl Into<String>) -> Self {
        self.values.push(value.into());
        self
    }

    /// Adds an excluded value, represented by RIS with a leading `-`.
    pub fn exclude(mut self, value: impl Into<String>) -> Self {
        let value = value.into();
        self.values.push(if value.starts_with('-') {
            value
        } else {
            format!("-{value}")
        });
        self
    }

    pub fn facet(&self) -> &RepositoryFacet {
        &self.facet
    }

    pub fn values(&self) -> &[String] {
        &self.values
    }
}

/// Information that helps construct the next package-hierarchy query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryPreselectionInfo {
    pub facet: RepositoryFacet,
    pub has_children_of_same_facet: bool,
}

/// One virtual folder returned by RIS.
#[derive(Clone, Debug)]
pub struct RepositoryVirtualFolder {
    /// The technical folder value, such as `CLAS`.
    pub name: String,
    /// The server-provided display label.
    pub display_name: String,
    /// The facet by which this folder groups its contents.
    pub facet: RepositoryFacet,
    /// The number of objects below this folder.
    pub object_count: u32,
    /// Additional server-provided text, often empty.
    pub text: String,
    /// Whether another hierarchy level uses the same facet.
    pub has_children_of_same_facet: bool,
    relations: Relations,
}

impl RepositoryVirtualFolder {
    /// Returns whether this folder selects objects assigned directly to a package.
    ///
    /// SAP prefixes the namespace-qualified package name with `..`, producing
    /// values such as `../DMO/FLIGHT_REUSE`.
    pub fn is_direct_assignment(&self) -> bool {
        self.direct_assignment_package().is_some()
    }

    /// Returns the package selected by a direct-assignment folder.
    pub fn direct_assignment_package(&self) -> Option<&str> {
        if self.facet != RepositoryFacet::PACKAGE {
            return None;
        }
        self.name
            .strip_prefix("..")
            .filter(|package| !package.is_empty())
    }

    /// Returns links advertised for this virtual folder.
    pub fn relations(&self) -> &Relations {
        &self.relations
    }

    /// Creates the filter selecting this folder in a subsequent hierarchy query.
    pub fn preselection(&self) -> RepositoryPreselection {
        RepositoryPreselection::new(self.facet.clone(), self.name.clone())
    }
}

/// A repository object listed in a virtual-folder result.
#[derive(Clone, Debug)]
pub struct RepositoryObjectEntry {
    pub name: String,
    /// The object version when the query requested version information.
    pub version: Option<String>,
    pub package: String,
    pub object_type: RepositoryObjectType,
    /// A validated, type-erased reference to the ADT object resource.
    pub reference: ObjectRef,
    /// The corresponding virtual Workbench URI, when supplied by SAP.
    pub virtual_workbench_uri: Option<String>,
    pub expandable: bool,
    /// The short description, omitted when descriptions were ignored.
    pub description: Option<String>,
    relations: Relations,
}

impl RepositoryObjectEntry {
    /// Returns links advertised for this repository object.
    pub fn relations(&self) -> &Relations {
        &self.relations
    }
}

/// The single hierarchy layer returned by a virtual-folder content query.
#[derive(Clone, Debug)]
pub struct RepositoryContent {
    pub object_count: u32,
    pub preselection_info: Option<RepositoryPreselectionInfo>,
    pub folders: Vec<RepositoryVirtualFolder>,
    pub objects: Vec<RepositoryObjectEntry>,
    relations: Relations,
}

impl RepositoryContent {
    pub(crate) fn parse(body: &[u8], request_uri: &AdtUri) -> Result<Self, RepositoryError> {
        let raw: RawRepositoryContent =
            serde_xml_rs::from_reader(body).map_err(RepositoryError::InvalidResponse)?;
        let query_reference = ObjectRef::new(request_uri.clone());
        let folders = raw
            .folders
            .into_iter()
            .map(|folder| RepositoryVirtualFolder {
                name: folder.name,
                display_name: folder.display_name,
                facet: folder.facet,
                object_count: folder.object_count,
                text: folder.text,
                has_children_of_same_facet: folder.has_children_of_same_facet,
                relations: Relations::new(query_reference.clone(), folder.links),
            })
            .collect();
        let objects = raw
            .objects
            .into_iter()
            .map(RepositoryObjectEntry::try_from)
            .collect::<Result<_, _>>()?;

        Ok(Self {
            object_count: raw.object_count,
            preselection_info: raw
                .preselection_info
                .map(|info| RepositoryPreselectionInfo {
                    facet: info.facet,
                    has_children_of_same_facet: info.has_children_of_same_facet,
                }),
            folders,
            objects,
            relations: Relations::new(query_reference, raw.links),
        })
    }

    /// Returns links advertised for the result as a whole.
    pub fn relations(&self) -> &Relations {
        &self.relations
    }
}

/// An optional URI-template link for discovering values of a facet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryFacetValuesLink {
    pub title: Option<String>,
    pub relation: String,
    pub template: String,
    pub media_type: Option<String>,
}

/// A facet advertised by the repository information system.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryFacetDefinition {
    /// The key exactly as advertised, commonly in lowercase.
    pub key: String,
    pub display_name: String,
    pub description: String,
    pub is_hierarchical: bool,
    pub is_for_filtering: bool,
    pub is_for_structuring: bool,
    pub values: Option<RepositoryFacetValuesLink>,
}

impl RepositoryFacetDefinition {
    /// Returns this advertised key in the uppercase form used by RIS queries.
    pub fn facet(&self) -> RepositoryFacet {
        self.key.to_ascii_uppercase().into()
    }
}

/// Facets supported by the repository information system.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryFacets {
    pub facets: Vec<RepositoryFacetDefinition>,
}

impl RepositoryFacets {
    pub(crate) fn parse(body: &[u8]) -> Result<Self, RepositoryError> {
        let raw: RawRepositoryFacets =
            serde_xml_rs::from_reader(body).map_err(RepositoryError::InvalidResponse)?;
        Ok(Self {
            facets: raw
                .facets
                .into_iter()
                .map(|facet| RepositoryFacetDefinition {
                    key: facet.key,
                    display_name: facet.display_name,
                    description: facet.description,
                    is_hierarchical: facet.is_hierarchical,
                    is_for_filtering: facet.is_for_filtering,
                    is_for_structuring: facet.is_for_structuring,
                    values: facet.values.map(|link| RepositoryFacetValuesLink {
                        title: link.title,
                        relation: link.relation,
                        template: link.template,
                        media_type: link.media_type,
                    }),
                })
                .collect(),
        })
    }
}

/// The RIS description of the object whose properties were requested.
#[derive(Clone, Debug)]
pub struct RepositoryObjectSummary {
    pub name: String,
    pub description: String,
    pub package: String,
    pub object_type: RepositoryObjectType,
    pub expandable: bool,
    pub reference: ObjectRef,
    relations: Relations,
}

impl RepositoryObjectSummary {
    /// Returns links advertised for this repository object.
    pub fn relations(&self) -> &Relations {
        &self.relations
    }
}

/// One facet value associated with a repository object.
#[derive(Clone, Debug)]
pub struct RepositoryProperty {
    pub facet: RepositoryFacet,
    pub value: String,
    pub display_name: String,
    pub description: Option<String>,
    pub has_children_of_same_facet: Option<bool>,
    relations: Relations,
}

impl RepositoryProperty {
    /// Returns links advertised for this property value.
    pub fn relations(&self) -> &Relations {
        &self.relations
    }
}

/// Uniform RIS properties for an arbitrary repository object.
#[derive(Clone, Debug)]
pub struct RepositoryObjectProperties {
    pub object: RepositoryObjectSummary,
    pub properties: Vec<RepositoryProperty>,
}

impl RepositoryObjectProperties {
    /// Returns the package hierarchy in the top-down order emitted by RIS.
    ///
    /// The first entry is the root package and the final entry is the package
    /// directly containing the object. An empty hierarchy means package
    /// properties were not requested or the object has no package assignment.
    pub fn package_hierarchy(&self) -> Result<Vec<ObjectRef<Package>>, ObjectError> {
        self.properties
            .iter()
            .filter(|property| property.facet == RepositoryFacet::PACKAGE)
            .map(|property| {
                let link = property.relations.find(PACKAGE_RELATION)?.ok_or(
                    ObjectError::MissingRelation {
                        relation: PACKAGE_RELATION,
                    },
                )?;
                ObjectRef::from_parts(property.value.clone(), link.target.clone())
            })
            .collect()
    }

    pub(crate) fn parse(body: &[u8], object_uri: &AdtUri) -> Result<Self, RepositoryError> {
        let raw: RawRepositoryObjectProperties =
            serde_xml_rs::from_reader(body).map_err(RepositoryError::InvalidResponse)?;
        let reference = ObjectRef::new(object_uri.clone());
        let properties = raw
            .properties
            .into_iter()
            .map(|property| RepositoryProperty {
                facet: property.facet,
                value: property.value,
                display_name: property.display_name,
                description: property.description,
                has_children_of_same_facet: property.has_children_of_same_facet,
                relations: Relations::new(reference.clone(), property.links),
            })
            .collect();
        let object = RepositoryObjectSummary {
            name: raw.object.name,
            description: raw.object.description,
            package: raw.object.package,
            object_type: raw.object.object_type,
            expandable: raw.object.expandable,
            relations: Relations::new(reference.clone(), raw.object.links),
            reference,
        };

        Ok(Self { object, properties })
    }
}

impl TryFrom<RawRepositoryObjectEntry> for RepositoryObjectEntry {
    type Error = RepositoryError;

    fn try_from(raw: RawRepositoryObjectEntry) -> Result<Self, Self::Error> {
        let uri = AdtUri::parse(&raw.uri).map_err(|source| RepositoryError::InvalidObjectUri {
            name: raw.name.clone(),
            uri: raw.uri,
            source,
        })?;
        let reference = ObjectRef::new(uri);
        Ok(Self {
            name: raw.name,
            version: raw.version,
            package: raw.package,
            object_type: raw.object_type,
            virtual_workbench_uri: raw.virtual_workbench_uri,
            expandable: raw.expandable,
            description: raw.description,
            relations: Relations::new(reference.clone(), raw.links),
            reference,
        })
    }
}

#[derive(Serialize)]
#[serde(rename = "vfs:virtualFoldersRequest")]
pub(crate) struct RepositoryContentRequest<'a> {
    #[serde(rename = "@objectSearchPattern")]
    search_pattern: &'a str,
    #[serde(rename = "vfs:preselection")]
    preselections: &'a [RepositoryPreselection],
    #[serde(rename = "vfs:facetorder")]
    facet_order: RawFacetOrder<'a>,
}

impl<'a> RepositoryContentRequest<'a> {
    pub(crate) fn new(
        search_pattern: &'a str,
        preselections: &'a [RepositoryPreselection],
        facets: &'a [RepositoryFacet],
    ) -> Self {
        Self {
            search_pattern,
            preselections,
            facet_order: RawFacetOrder { facets },
        }
    }

    pub(crate) fn serialize(&self) -> Result<String, RepositoryError> {
        serde_xml_rs::SerdeXml::new()
            .namespace("vfs", VIRTUAL_FOLDERS_NAMESPACE)
            .to_string(self)
            .map_err(RepositoryError::InvalidRequest)
    }
}

#[derive(Serialize)]
struct RawFacetOrder<'a> {
    #[serde(rename = "vfs:facet")]
    facets: &'a [RepositoryFacet],
}

#[derive(Deserialize)]
#[serde(rename = "vfs:virtualFoldersResult")]
struct RawRepositoryContent {
    #[serde(rename = "@objectCount")]
    object_count: u32,
    #[serde(rename = "vfs:preselectionInfo")]
    preselection_info: Option<RawPreselectionInfo>,
    #[serde(rename = "vfs:virtualFolder", default)]
    folders: Vec<RawVirtualFolder>,
    #[serde(rename = "vfs:object", default)]
    objects: Vec<RawRepositoryObjectEntry>,
    #[serde(rename = "atom:link", default)]
    links: Vec<AdvertisedLink>,
}

#[derive(Deserialize)]
struct RawPreselectionInfo {
    #[serde(rename = "@facet")]
    facet: RepositoryFacet,
    #[serde(rename = "@hasChildrenOfSameFacet")]
    has_children_of_same_facet: bool,
}

#[derive(Deserialize)]
struct RawVirtualFolder {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@displayName")]
    display_name: String,
    #[serde(rename = "@facet")]
    facet: RepositoryFacet,
    #[serde(rename = "@counter")]
    object_count: u32,
    #[serde(rename = "@text", default)]
    text: String,
    #[serde(rename = "@hasChildrenOfSameFacet")]
    has_children_of_same_facet: bool,
    #[serde(rename = "atom:link", default)]
    links: Vec<AdvertisedLink>,
}

#[derive(Deserialize)]
struct RawRepositoryObjectEntry {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@version")]
    version: Option<String>,
    #[serde(rename = "@package")]
    package: String,
    #[serde(rename = "@type")]
    object_type: RepositoryObjectType,
    #[serde(rename = "@uri")]
    uri: String,
    #[serde(rename = "@vituri")]
    virtual_workbench_uri: Option<String>,
    #[serde(rename = "@expandable")]
    expandable: bool,
    #[serde(rename = "@text")]
    description: Option<String>,
    #[serde(rename = "atom:link", default)]
    links: Vec<AdvertisedLink>,
}

#[derive(Deserialize)]
#[serde(rename = "vf:facets")]
struct RawRepositoryFacets {
    #[serde(rename = "vf:facet", default)]
    facets: Vec<RawRepositoryFacetDefinition>,
}

#[derive(Deserialize)]
struct RawRepositoryFacetDefinition {
    #[serde(rename = "@key")]
    key: String,
    #[serde(rename = "@displayName")]
    display_name: String,
    #[serde(rename = "@description")]
    description: String,
    #[serde(rename = "@isHierarchical")]
    is_hierarchical: bool,
    #[serde(rename = "@isForFiltering")]
    is_for_filtering: bool,
    #[serde(rename = "@isForStructuring")]
    is_for_structuring: bool,
    #[serde(rename = "adtcomp:templateLink")]
    values: Option<RawTemplateLink>,
}

#[derive(Deserialize)]
struct RawTemplateLink {
    #[serde(rename = "@title")]
    title: Option<String>,
    #[serde(rename = "@rel")]
    relation: String,
    #[serde(rename = "@template")]
    template: String,
    #[serde(rename = "@type")]
    media_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename = "opr:objectProperties")]
struct RawRepositoryObjectProperties {
    #[serde(rename = "opr:object")]
    object: RawRepositoryObjectSummary,
    #[serde(rename = "opr:property", default)]
    properties: Vec<RawRepositoryProperty>,
}

#[derive(Deserialize)]
struct RawRepositoryObjectSummary {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@text", default)]
    description: String,
    #[serde(rename = "@package")]
    package: String,
    #[serde(rename = "@type")]
    object_type: RepositoryObjectType,
    #[serde(rename = "@expandable")]
    expandable: bool,
    #[serde(rename = "atom:link", default)]
    links: Vec<AdvertisedLink>,
}

#[derive(Deserialize)]
struct RawRepositoryProperty {
    #[serde(rename = "@facet")]
    facet: RepositoryFacet,
    #[serde(rename = "@name")]
    value: String,
    #[serde(rename = "@displayName")]
    display_name: String,
    #[serde(rename = "@text")]
    description: Option<String>,
    #[serde(rename = "@hasChildrenOfSameFacet")]
    has_children_of_same_facet: Option<bool>,
    #[serde(rename = "atom:link", default)]
    links: Vec<AdvertisedLink>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTENT_XML: &[u8] = include_bytes!("../../tests/fixtures/repository-content.xml");
    const FACETS_XML: &[u8] = include_bytes!("../../tests/fixtures/repository-facets.xml");
    const OBJECT_PROPERTIES_XML: &[u8] =
        include_bytes!("../../tests/fixtures/repository-object-properties.xml");
    const OBJECT_PROPERTIES_HIERARCHY_XML: &[u8] =
        include_bytes!("../../tests/fixtures/repository-object-properties-hierarchy.xml");

    #[test]
    fn serializes_virtual_folder_filters() {
        let selections = [
            RepositoryPreselection::new(RepositoryFacet::OWNER, "DEVELOPER").include("JOHN DOE"),
            RepositoryPreselection::new(RepositoryFacet::PACKAGE, "$TMP").exclude("UI5/STRU"),
        ];
        let facets = [RepositoryFacet::GROUP, RepositoryFacet::TYPE];

        let xml = RepositoryContentRequest::new("*", &selections, &facets)
            .serialize()
            .unwrap();

        assert!(xml.contains("objectSearchPattern=\"*\""));
        assert!(xml.contains("<vfs:value>JOHN DOE</vfs:value>"));
        assert!(xml.contains("<vfs:value>-UI5/STRU</vfs:value>"));
        assert!(xml.contains("<vfs:facet>GROUP</vfs:facet>"));
        assert!(xml.contains("<vfs:facet>TYPE</vfs:facet>"));
    }

    #[test]
    fn creates_direct_package_preselections() {
        for package in ["/DMO/FLIGHT", "ZPACKAGE", "$TMP", "../DMO/FLIGHT"] {
            let selection = RepositoryPreselection::direct_package(package);

            assert_eq!(selection.facet(), &RepositoryFacet::PACKAGE);
            assert_eq!(
                selection.values(),
                [format!("..{}", package.trim_start_matches(".."))]
            );
        }
    }

    #[test]
    fn parses_virtual_folders_and_repository_objects() {
        let base =
            AdtUri::parse("/sap/bc/adt/repository/informationsystem/virtualfolders/contents")
                .unwrap();

        let content = RepositoryContent::parse(CONTENT_XML, &base).unwrap();

        assert_eq!(content.object_count, 3);
        assert_eq!(
            content.preselection_info.unwrap().facet,
            RepositoryFacet::PACKAGE
        );
        assert_eq!(content.folders[0].name, "SOURCE_LIBRARY");
        assert!(!content.folders[0].is_direct_assignment());
        assert_eq!(content.folders[0].relations().len(), 1);
        assert_eq!(
            content.folders[0].preselection().values(),
            ["SOURCE_LIBRARY"]
        );
        assert_eq!(content.objects[0].name, "ZCL_DEMO");
        assert_eq!(content.objects[0].object_type.to_string(), "CLAS/OC");
        assert_eq!(
            content.objects[0].reference.uri().as_str(),
            "/sap/bc/adt/oo/classes/zcl_demo"
        );
        assert_eq!(
            content.objects[0]
                .relations()
                .iter()
                .next()
                .unwrap()
                .unwrap()
                .target,
            *content.objects[0].reference.uri()
        );
    }

    #[test]
    fn identifies_direct_package_assignment_folders() {
        let base =
            AdtUri::parse("/sap/bc/adt/repository/informationsystem/virtualfolders/contents")
                .unwrap();

        for package in ["/DMO/FLIGHT", "ZPACKAGE", "$TMP"] {
            let xml = String::from_utf8(CONTENT_XML.to_vec())
                .unwrap()
                .replace("name=\"SOURCE_LIBRARY\"", &format!("name=\"..{package}\""))
                .replace("facet=\"GROUP\"", "facet=\"PACKAGE\"");
            let content = RepositoryContent::parse(xml.as_bytes(), &base).unwrap();

            assert!(content.folders[0].is_direct_assignment());
            assert_eq!(
                content.folders[0].direct_assignment_package(),
                Some(package)
            );
        }
    }

    #[test]
    fn does_not_treat_other_facets_as_direct_package_assignments() {
        let xml = String::from_utf8(CONTENT_XML.to_vec())
            .unwrap()
            .replace("name=\"SOURCE_LIBRARY\"", "name=\"..SOURCE_LIBRARY\"");
        let base =
            AdtUri::parse("/sap/bc/adt/repository/informationsystem/virtualfolders/contents")
                .unwrap();

        let content = RepositoryContent::parse(xml.as_bytes(), &base).unwrap();

        assert!(!content.folders[0].is_direct_assignment());
        assert_eq!(content.folders[0].direct_assignment_package(), None);
    }

    #[test]
    fn preserves_custom_facets_and_repository_types() {
        let xml = String::from_utf8(CONTENT_XML.to_vec())
            .unwrap()
            .replace("facet=\"GROUP\"", "facet=\"FUTURE\"")
            .replace("type=\"CLAS/OC\"", "type=\"ZZZZ/X\"");
        let base =
            AdtUri::parse("/sap/bc/adt/repository/informationsystem/virtualfolders/contents")
                .unwrap();

        let content = RepositoryContent::parse(xml.as_bytes(), &base).unwrap();

        assert_eq!(content.folders[0].facet.as_str(), "FUTURE");
        assert_eq!(content.objects[0].object_type.to_string(), "ZZZZ/X");
        assert_eq!(
            content.objects[0]
                .object_type
                .global_workbench_type()
                .unwrap()
                .to_string(),
            "ZZZZ/X"
        );
    }

    #[test]
    fn accepts_four_character_ris_type_aliases() {
        let xml = String::from_utf8(CONTENT_XML.to_vec())
            .unwrap()
            .replace("type=\"CLAS/OC\"", "type=\"AUTH\"");
        let base =
            AdtUri::parse("/sap/bc/adt/repository/informationsystem/virtualfolders/contents")
                .unwrap();

        let content = RepositoryContent::parse(xml.as_bytes(), &base).unwrap();

        assert_eq!(content.objects[0].object_type.as_str(), "AUTH");
        assert!(
            content.objects[0]
                .object_type
                .global_workbench_type()
                .is_err()
        );
    }

    #[test]
    fn rejects_repository_object_uris_outside_the_sap_namespace() {
        let xml = String::from_utf8(CONTENT_XML.to_vec()).unwrap().replace(
            "/sap/bc/adt/oo/classes/zcl_demo",
            "https://attacker.example/sap/bc/adt/oo/classes/zcl_demo",
        );
        let base =
            AdtUri::parse("/sap/bc/adt/repository/informationsystem/virtualfolders/contents")
                .unwrap();

        let error = RepositoryContent::parse(xml.as_bytes(), &base).unwrap_err();

        assert!(matches!(error, RepositoryError::InvalidObjectUri { .. }));
    }

    #[test]
    fn parses_available_facets_and_value_templates() {
        let facets = RepositoryFacets::parse(FACETS_XML).unwrap();

        assert_eq!(facets.facets.len(), 2);
        assert_eq!(facets.facets[0].key, "appl");
        assert_eq!(
            facets.facets[0].facet(),
            RepositoryFacet::APPLICATION_COMPONENT
        );
        assert!(facets.facets[0].is_hierarchical);
        assert_eq!(
            facets.facets[0].values.as_ref().unwrap().template,
            "/sap/bc/adt/repository/informationsystem/properties/values?data=appl{&name}"
        );
        assert!(facets.facets[1].values.is_none());
    }

    #[test]
    fn parses_uniform_object_properties() {
        let object_uri = AdtUri::parse("/sap/bc/adt/oo/classes/cl_adt_uri_mapper").unwrap();

        let properties =
            RepositoryObjectProperties::parse(OBJECT_PROPERTIES_XML, &object_uri).unwrap();

        assert_eq!(properties.object.name, "CL_ADT_URI_MAPPER");
        assert_eq!(properties.object.object_type.to_string(), "CLAS/OC");
        assert_eq!(properties.object.reference.uri(), &object_uri);
        assert_eq!(properties.object.relations().len(), 1);
        assert_eq!(properties.properties[0].facet, RepositoryFacet::PACKAGE);
        let package_hierarchy = properties.package_hierarchy().unwrap();
        let package = &package_hierarchy[0];
        assert_eq!(package.name(), "SADT_TOOLS_CORE");
        assert_eq!(
            package.uri().as_str(),
            "/sap/bc/adt/packages/sadt_tools_core"
        );
        assert_eq!(properties.properties[0].value, "SADT_TOOLS_CORE");
        assert_eq!(properties.properties[0].relations().len(), 1);
        assert_eq!(properties.properties[2].facet.as_str(), "FUTURE");
    }

    #[test]
    fn returns_the_complete_package_hierarchy_in_response_order() {
        let object_uri =
            AdtUri::parse("/sap/bc/adt/oo/classes/cl_ris_adt_res_obj_properties").unwrap();
        let properties =
            RepositoryObjectProperties::parse(OBJECT_PROPERTIES_HIERARCHY_XML, &object_uri)
                .unwrap();

        let hierarchy = properties.package_hierarchy().unwrap();

        assert_eq!(
            hierarchy.iter().map(ObjectRef::name).collect::<Vec<_>>(),
            ["BASIS", "SRIS", "SRIS_ADT"]
        );
        assert_eq!(hierarchy[0].uri().as_str(), "/sap/bc/adt/packages/basis");
        assert_eq!(
            hierarchy.last().unwrap().uri().as_str(),
            "/sap/bc/adt/packages/sris_adt"
        );
        assert_eq!(
            properties
                .properties
                .iter()
                .filter(|property| property.facet == RepositoryFacet::APPLICATION_COMPONENT)
                .map(|property| property.value.as_str())
                .collect::<Vec<_>>(),
            ["BC", "BC-DWB", "BC-DWB-AIE"]
        );
    }

    #[test]
    fn package_hierarchy_requires_a_relation_for_every_level() {
        let xml = String::from_utf8(OBJECT_PROPERTIES_HIERARCHY_XML.to_vec())
            .unwrap()
            .replacen(PACKAGE_RELATION, "urn:unexpected", 1);
        let object_uri =
            AdtUri::parse("/sap/bc/adt/oo/classes/cl_ris_adt_res_obj_properties").unwrap();
        let properties = RepositoryObjectProperties::parse(xml.as_bytes(), &object_uri).unwrap();

        let error = properties.package_hierarchy().unwrap_err();

        assert!(matches!(
            error,
            ObjectError::MissingRelation {
                relation: PACKAGE_RELATION
            }
        ));
    }
}
