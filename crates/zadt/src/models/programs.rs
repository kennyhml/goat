use serde::Deserialize;

use crate::{
    AdtUri, EnhancementImplementationsRef, EntityTag, GlobalWorkbenchType, HtmlSourceRef, Include,
    NegotiableMediaVersion, ObjectEnhancementOptionsRef, ObjectError, ObjectRef, ObjectStateRef,
    ObjectStructureRef, ObjectType, ObjectVersion, ParserRef, Program, ResponseError,
    SourceEnhancementOptionsRef, SourceRef, SourceVersionsRef, TextElementsRef,
    objects::Package,
    resource::{AdtLinkError, AdvertisedLink, Relations, resolve_href},
};

/// The SAP media-type version used to decode program properties.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ProgramPropertiesVersion {
    media_type: &'static str,
    kind: ProgramPropertiesVersionKind,
}

impl NegotiableMediaVersion for ProgramPropertiesVersion {
    const SUPPORTED: &'static [Self] = &[Self::V3, Self::V2];

    fn media_type(self) -> &'static str {
        self.media_type
    }
}

impl ProgramPropertiesVersion {
    pub const V2: Self = Self {
        media_type: "application/vnd.sap.adt.programs.programs.v2+xml",
        kind: ProgramPropertiesVersionKind::V2,
    };

    pub const V3: Self = Self {
        media_type: "application/vnd.sap.adt.programs.programs.v3+xml",
        kind: ProgramPropertiesVersionKind::V3,
    };
}

/// Local helper to ensure exhaustive matching
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ProgramPropertiesVersionKind {
    V2,
    V3,
}

/// Properties of a program.
///
/// Multiple media type versions exist. They do, however, appear to be
/// identical under regular circumstances - to be clarified.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum ProgramProperties {
    V2(Box<ProgramPropertiesV2>),
    V3(Box<ProgramPropertiesV3>),
}

impl ProgramProperties {
    /// Returns the response media-type version.
    pub fn media_version(&self) -> ProgramPropertiesVersion {
        match self {
            Self::V2(_) => ProgramPropertiesVersion::V2,
            Self::V3(_) => ProgramPropertiesVersion::V3,
        }
    }

    /// Returns the response entity tag, when present.
    pub fn etag(&self) -> Option<&EntityTag> {
        match self {
            Self::V2(program) | Self::V3(program) => program.etag.as_ref(),
        }
    }

    pub(crate) fn parse(
        resource: &ObjectRef<Program>,
        media_version: ProgramPropertiesVersion,
        body: &[u8],
        etag: Option<EntityTag>,
    ) -> Result<Self, ResponseError> {
        let parsed: RawProgramProperties =
            serde_xml_rs::from_reader(body).map_err(ObjectError::InvalidResponse)?;
        let properties = ProgramPropertiesV3::from_raw(resource.clone(), parsed, etag)?;
        Ok(match media_version.kind {
            ProgramPropertiesVersionKind::V2 => Self::V2(Box::new(properties)),
            ProgramPropertiesVersionKind::V3 => Self::V3(Box::new(properties)),
        })
    }
}

/// The SAP media-type version used to decode include properties.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IncludePropertyVersion {
    V2,
}

impl NegotiableMediaVersion for IncludePropertyVersion {
    const SUPPORTED: &'static [Self] = &[Self::V2];

    fn media_type(self) -> &'static str {
        match self {
            Self::V2 => "application/vnd.sap.adt.programs.includes.v2+xml",
        }
    }
}

/// Include properties tagged with the media-type version returned by ADT.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum IncludeProperties {
    V2(Box<IncludePropertiesV2>),
}

impl IncludeProperties {
    /// Returns the response media-type version.
    pub fn media_version(&self) -> IncludePropertyVersion {
        match self {
            Self::V2(_) => IncludePropertyVersion::V2,
        }
    }

    /// Returns the response entity tag, when present.
    pub fn etag(&self) -> Option<&str> {
        match self {
            Self::V2(include) => include.etag.as_deref(),
        }
    }

    pub(crate) fn parse(
        resource: &ObjectRef<Include>,
        version: IncludePropertyVersion,
        body: &[u8],
        etag: Option<EntityTag>,
    ) -> Result<Self, ResponseError> {
        let parsed: RawIncludeProperties =
            serde_xml_rs::from_reader(body).map_err(ObjectError::InvalidResponse)?;
        let properties = IncludePropertiesV2::from_raw(resource.clone(), parsed, etag)?;
        Ok(match version {
            IncludePropertyVersion::V2 => Self::V2(Box::new(properties)),
        })
    }
}

/// The plain-text console output produced by running an ABAP program.
#[derive(Clone, Debug, Eq, PartialEq)]
#[readonly::make]
pub struct ProgramRunResult {
    /// The program that was executed.
    pub reference: ObjectRef<Program>,

    /// The rendered program output returned by SAP.
    pub content: String,
}

impl ProgramRunResult {
    pub(crate) fn new(reference: ObjectRef<Program>, content: String) -> Self {
        Self { reference, content }
    }
}

/// The V2 program-properties representation uses the V3 payload schema.
pub type ProgramPropertiesV2 = ProgramPropertiesV3;

/// The ABAP program-properties payload shared by the V2 and V3 media types.
#[derive(Clone, Debug)]
#[readonly::make]
pub struct ProgramPropertiesV3 {
    /// The program resource that was fetched.
    pub reference: ObjectRef<Program>,

    /// The program name supplied by SAP.
    pub name: String,

    /// The repository object type, normally `PROG/P`.
    pub object_type: GlobalWorkbenchType,

    /// The timestamp at which the program was last changed.
    pub last_changed: String,

    /// The object state, such as `active` or `inactive`.
    pub version: ObjectVersion,

    /// The timestamp at which the program was created.
    pub created_at: String,

    /// The user who last changed the program.
    pub changed_by: String,

    /// The program description.
    pub description: String,

    /// The maximum length of the program description.
    pub description_text_limit: u32,

    /// The program's logon language.
    pub language: String,

    /// Whether this program is locked by the current editor.
    pub locked_by_editor: bool,

    /// The semantic program type, such as `executableProgram`.
    pub program_type: String,

    /// Whether fixed-point arithmetic is enabled.
    pub fix_point_arithmetic: bool,

    /// Whether the active Unicode check is enabled.
    pub unicode_check_active: bool,

    /// The user responsible for the program.
    pub responsible: String,

    /// The program's master language.
    pub master_language: String,

    /// The program's master system.
    pub master_system: String,

    /// The configured ABAP language version.
    pub abap_language_version: String,

    /// The package containing the program.
    pub package: ObjectRef<Package>,

    /// The syntax configuration and parser advertised for the source.
    pub syntax_configuration: SyntaxConfiguration,

    /// The advertised plain-text source representation.
    pub source: SourceRef,

    /// The entity tag of these program properties, when present.
    pub etag: Option<EntityTag>,

    relations: Relations,
}

impl ProgramPropertiesV3 {
    /// Returns the program's advertised links without resolving them eagerly.
    pub fn relations(&self) -> &Relations {
        &self.relations
    }

    /// Resolves the advertised rendered HTML source, when present.
    pub fn html_source(&self) -> Result<Option<HtmlSourceRef>, AdtLinkError> {
        self.relations.get()
    }

    /// Resolves the advertised source version-history resource, when present.
    pub fn versions(&self) -> Result<Option<SourceVersionsRef>, AdtLinkError> {
        self.relations.get()
    }

    /// Resolves the advertised object-structure resource, when present.
    pub fn object_structure(&self) -> Result<Option<ObjectStructureRef>, AdtLinkError> {
        self.relations.get()
    }

    /// Resolves the advertised text-elements resource, when present.
    pub fn text_elements(&self) -> Result<Option<TextElementsRef>, AdtLinkError> {
        self.relations.get()
    }

    /// Resolves the advertised enhancement implementations, when present.
    pub fn enhancement_implementations(
        &self,
    ) -> Result<Option<EnhancementImplementationsRef>, AdtLinkError> {
        self.relations.get()
    }

    /// Resolves the advertised object enhancement options, when present.
    pub fn enhancement_options(&self) -> Result<Option<ObjectEnhancementOptionsRef>, AdtLinkError> {
        self.relations.get()
    }

    /// Resolves the advertised source enhancement options, when present.
    pub fn source_enhancement_options(
        &self,
    ) -> Result<Option<SourceEnhancementOptionsRef>, AdtLinkError> {
        self.relations.get()
    }

    /// Resolves the link to the program's other active or inactive state.
    pub fn object_state(&self) -> Result<Option<ObjectStateRef>, AdtLinkError> {
        self.relations.get()
    }

    fn from_raw(
        reference: ObjectRef<Program>,
        raw: RawProgramProperties,
        etag: Option<EntityTag>,
    ) -> Result<Self, ObjectError> {
        let package = package_reference(raw.package)?;

        let version = ObjectVersion::parse(&raw.version).ok_or_else(|| {
            ObjectError::UnsupportedObjectVersion {
                version: raw.version.clone(),
            }
        })?;
        let relations = Relations::new(reference.erase(), raw.links);
        let source: SourceRef = relations.get()?.ok_or(ObjectError::MissingRelation {
            relation: "plain-text source",
        })?;
        let declared_source = resolve_href(reference.uri(), &raw.source_uri).map_err(|source| {
            ObjectError::InvalidLink {
                href: raw.source_uri.clone(),
                source,
            }
        })?;
        if declared_source.target != source.uri {
            return Err(ObjectError::RelationMismatch {
                relation: "source",
                declared: declared_source.target.to_string(),
                advertised: source.uri.to_string(),
            });
        }

        let syntax_relations =
            Relations::new(reference.erase(), raw.syntax_configuration.language.links);

        Ok(Self {
            reference,
            name: raw.name,
            object_type: raw.object_type,
            last_changed: raw.last_changed,
            version,
            created_at: raw.created_at,
            changed_by: raw.changed_by,
            description: raw.description,
            description_text_limit: raw.description_text_limit,
            language: raw.language,
            locked_by_editor: raw.locked_by_editor,
            program_type: raw.program_type,
            fix_point_arithmetic: raw.fix_point_arithmetic,
            unicode_check_active: raw.unicode_check_active,
            responsible: raw.responsible,
            master_language: raw.master_language,
            master_system: raw.master_system,
            abap_language_version: raw.abap_language_version,
            package,
            syntax_configuration: SyntaxConfiguration {
                language: SyntaxLanguage {
                    version: raw.syntax_configuration.language.version,
                    description: raw.syntax_configuration.language.description,
                    relations: syntax_relations,
                },
            },
            source,
            etag,
            relations,
        })
    }
}

/// The V2 standalone ABAP include-properties payload.
#[derive(Clone, Debug)]
#[readonly::make]
pub struct IncludePropertiesV2 {
    /// The include resource that was fetched.
    pub reference: ObjectRef<Include>,

    /// The include name supplied by SAP.
    pub name: String,

    /// The repository object type, normally `PROG/I`.
    pub object_type: GlobalWorkbenchType,

    /// The timestamp at which the include was last changed.
    pub last_changed: String,

    /// The object state, such as `active` or `inactive`.
    pub version: ObjectVersion,

    /// The timestamp at which the include was created.
    pub created_at: String,

    /// The user who last changed the include.
    pub changed_by: String,

    /// The include description.
    pub description: String,

    /// The maximum length of the include description.
    pub description_text_limit: u32,

    /// The include's logon language.
    pub language: String,

    /// Number of objects reported as using this include.
    pub context_ref_count: u32,

    /// The using object when SAP reports exactly one context.
    pub context_ref: Option<ObjectRef>,

    /// Whether fixed-point arithmetic is enabled.
    pub fix_point_arithmetic: bool,

    /// Whether the active Unicode check is enabled.
    pub unicode_check_active: bool,

    /// The user responsible for the include.
    pub responsible: String,

    /// The include's master language.
    pub master_language: String,

    /// The include's master system.
    pub master_system: String,

    /// The package containing the include.
    pub package: ObjectRef<Package>,

    /// The advertised plain-text source representation.
    pub source: SourceRef,

    /// The entity tag of these include properties, when present.
    pub etag: Option<EntityTag>,

    relations: Relations,
}

impl IncludePropertiesV2 {
    /// Returns the include's advertised links without resolving them eagerly.
    pub fn relations(&self) -> &Relations {
        &self.relations
    }

    /// Resolves the advertised rendered HTML source, when present.
    pub fn html_source(&self) -> Result<Option<HtmlSourceRef>, AdtLinkError> {
        self.relations.get()
    }

    /// Resolves the advertised source version-history resource, when present.
    pub fn versions(&self) -> Result<Option<SourceVersionsRef>, AdtLinkError> {
        self.relations.get()
    }

    /// Resolves the advertised text-elements resource, when present.
    pub fn text_elements(&self) -> Result<Option<TextElementsRef>, AdtLinkError> {
        self.relations.get()
    }

    /// Resolves the advertised enhancement implementations, when present.
    pub fn enhancement_implementations(
        &self,
    ) -> Result<Option<EnhancementImplementationsRef>, AdtLinkError> {
        self.relations.get()
    }

    /// Resolves the advertised object enhancement options, when present.
    pub fn enhancement_options(&self) -> Result<Option<ObjectEnhancementOptionsRef>, AdtLinkError> {
        self.relations.get()
    }

    /// Resolves the advertised source enhancement options, when present.
    pub fn source_enhancement_options(
        &self,
    ) -> Result<Option<SourceEnhancementOptionsRef>, AdtLinkError> {
        self.relations.get()
    }

    fn from_raw(
        reference: ObjectRef<Include>,
        raw: RawIncludeProperties,
        etag: Option<EntityTag>,
    ) -> Result<Self, ObjectError> {
        let package = package_reference(raw.package)?;

        let version = ObjectVersion::parse(&raw.version).ok_or_else(|| {
            ObjectError::UnsupportedObjectVersion {
                version: raw.version.clone(),
            }
        })?;
        let relations = Relations::new(reference.erase(), raw.links);
        let source: SourceRef = relations.get()?.ok_or(ObjectError::MissingRelation {
            relation: "plain-text source",
        })?;
        let declared_source = resolve_href(reference.uri(), &raw.source_uri).map_err(|source| {
            ObjectError::InvalidLink {
                href: raw.source_uri.clone(),
                source,
            }
        })?;
        if declared_source.target != source.uri {
            return Err(ObjectError::RelationMismatch {
                relation: "source",
                declared: declared_source.target.to_string(),
                advertised: source.uri.to_string(),
            });
        }

        let context_ref = raw
            .context_ref
            .map(|context| {
                resolve_href(reference.uri(), &context.uri)
                    .map(|resolved| ObjectRef::new(resolved.target))
                    .map_err(|source| ObjectError::InvalidLink {
                        href: context.uri,
                        source,
                    })
            })
            .transpose()?;
        Ok(Self {
            reference,
            name: raw.name,
            object_type: raw.object_type,
            last_changed: raw.last_changed,
            version,
            created_at: raw.created_at,
            changed_by: raw.changed_by,
            description: raw.description,
            description_text_limit: raw.description_text_limit,
            language: raw.language,
            context_ref_count: raw.context_ref_count,
            context_ref,
            fix_point_arithmetic: raw.fix_point_arithmetic,
            unicode_check_active: raw.unicode_check_active,
            responsible: raw.responsible,
            master_language: raw.master_language,
            master_system: raw.master_system,
            package,
            source,
            etag,
            relations,
        })
    }
}

/// The source parser configuration advertised by a program.
#[derive(Clone, Debug, Eq, PartialEq)]
#[readonly::make]
pub struct SyntaxConfiguration {
    /// The configured ABAP language.
    pub language: SyntaxLanguage,
}

/// An ABAP language version, description, and optional parser grammar.
#[derive(Clone, Debug, Eq, PartialEq)]
#[readonly::make]
pub struct SyntaxLanguage {
    /// The language version identifier, such as `X`.
    pub version: String,

    /// The server-provided language description.
    pub description: String,

    relations: Relations,
}

impl SyntaxLanguage {
    /// Returns the syntax language's advertised links without resolving them eagerly.
    pub fn relations(&self) -> &Relations {
        &self.relations
    }

    /// Resolves the advertised parser grammar, when present.
    pub fn parser(&self) -> Result<Option<ParserRef>, AdtLinkError> {
        self.relations.get()
    }
}

#[derive(Deserialize)]
#[serde(rename = "program:abapProgram")]
struct RawProgramProperties {
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
    #[serde(rename = "@adtcore:description")]
    description: String,
    #[serde(rename = "@adtcore:descriptionTextLimit")]
    description_text_limit: u32,
    #[serde(rename = "@adtcore:language")]
    language: String,
    #[serde(rename = "@program:lockedByEditor")]
    locked_by_editor: bool,
    #[serde(rename = "@program:programType")]
    program_type: String,
    #[serde(rename = "@abapsource:sourceUri")]
    source_uri: String,
    #[serde(rename = "@abapsource:fixPointArithmetic")]
    fix_point_arithmetic: bool,
    #[serde(rename = "@abapsource:activeUnicodeCheck")]
    unicode_check_active: bool,
    #[serde(rename = "@adtcore:responsible")]
    responsible: String,
    #[serde(rename = "@adtcore:masterLanguage")]
    master_language: String,
    #[serde(rename = "@adtcore:masterSystem")]
    master_system: String,
    #[serde(rename = "@adtcore:abapLanguageVersion")]
    abap_language_version: String,
    #[serde(rename = "adtcore:packageRef")]
    package: RawPackage,
    #[serde(rename = "abapsource:syntaxConfiguration")]
    syntax_configuration: RawSyntaxConfiguration,
    #[serde(rename = "atom:link", default)]
    links: Vec<AdvertisedLink>,
}

#[derive(Deserialize)]
#[serde(rename = "include:abapInclude")]
struct RawIncludeProperties {
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
    #[serde(rename = "@adtcore:description")]
    description: String,
    #[serde(rename = "@adtcore:descriptionTextLimit")]
    description_text_limit: u32,
    #[serde(rename = "@adtcore:language")]
    language: String,
    #[serde(rename = "@include:contextRefCount", default)]
    context_ref_count: u32,
    #[serde(rename = "@abapsource:sourceUri")]
    source_uri: String,
    #[serde(rename = "@abapsource:fixPointArithmetic")]
    fix_point_arithmetic: bool,
    #[serde(rename = "@abapsource:activeUnicodeCheck")]
    unicode_check_active: bool,
    #[serde(rename = "@adtcore:responsible")]
    responsible: String,
    #[serde(rename = "@adtcore:masterLanguage")]
    master_language: String,
    #[serde(rename = "@adtcore:masterSystem")]
    master_system: String,
    #[serde(rename = "adtcore:packageRef")]
    package: RawPackage,
    #[serde(rename = "include:contextRef")]
    context_ref: Option<RawObjectReference>,
    #[serde(rename = "atom:link", default)]
    links: Vec<AdvertisedLink>,
}

#[derive(Deserialize)]
struct RawObjectReference {
    #[serde(rename = "@adtcore:uri")]
    uri: String,
}

#[derive(Deserialize)]
struct RawPackage {
    #[serde(rename = "@adtcore:name")]
    name: String,
    #[serde(rename = "@adtcore:uri")]
    uri: String,
    #[serde(rename = "@adtcore:type")]
    object_type: GlobalWorkbenchType,
}

fn package_reference(raw: RawPackage) -> Result<ObjectRef<Package>, ObjectError> {
    if raw.object_type != Package::WORKBENCH_TYPE {
        return Err(ObjectError::UnexpectedObjectType {
            expected: Package::WORKBENCH_TYPE,
            actual: raw.object_type,
        });
    }
    let uri = AdtUri::parse(&raw.uri).map_err(|source| ObjectError::InvalidLink {
        href: raw.uri.clone(),
        source,
    })?;
    ObjectRef::from_parts(raw.name, uri)
}

#[derive(Deserialize)]
struct RawSyntaxConfiguration {
    #[serde(rename = "abapsource:language")]
    language: RawSyntaxLanguage,
}

#[derive(Deserialize)]
struct RawSyntaxLanguage {
    #[serde(rename = "abapsource:version")]
    version: String,
    #[serde(rename = "abapsource:description")]
    description: String,
    #[serde(rename = "atom:link", default)]
    links: Vec<AdvertisedLink>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROGRAM_XML: &str = include_str!("../../tests/fixtures/program-z-test.xml");
    const INCLUDE_XML: &str = include_str!("../../tests/fixtures/include-ztest.xml");

    fn parse(body: &str) -> Result<ProgramPropertiesV3, ResponseError> {
        let properties = ProgramProperties::parse(
            &ObjectRef::<Program>::for_test(
                "Z_TEST",
                crate::AdtUri::parse("/sap/bc/adt/programs/programs/Z_TEST").unwrap(),
            ),
            ProgramPropertiesVersion::V3,
            body.as_bytes(),
            Some(EntityTag::from_static("program-etag")),
        )?;
        Ok(match properties {
            ProgramProperties::V2(properties) | ProgramProperties::V3(properties) => *properties,
        })
    }

    fn assert_program(program: &ProgramPropertiesV3) {
        assert_eq!(program.name, "Z_TEST");
        assert_eq!(program.version, ObjectVersion::Inactive);
        assert_eq!(program.etag.as_deref(), Some("program-etag"));
        assert_eq!(
            program.source.uri.as_str(),
            "/sap/bc/adt/programs/programs/Z_TEST/source/main"
        );
        assert_eq!(program.source.etag.as_deref(), Some("202607251959580001"));
        assert_eq!(program.relations().len(), 9);
        assert_eq!(program.syntax_configuration.language.relations().len(), 1);
        assert_eq!(
            program
                .syntax_configuration
                .language
                .parser()
                .unwrap()
                .unwrap()
                .etag
                .as_deref(),
            Some("757")
        );
    }

    #[test]
    fn parses_include_properties() {
        let reference = ObjectRef::<Include>::for_test(
            "ZTEST",
            crate::AdtUri::parse("/sap/bc/adt/programs/includes/ZTEST").unwrap(),
        );
        let properties = IncludeProperties::parse(
            &reference,
            IncludePropertyVersion::V2,
            INCLUDE_XML.as_bytes(),
            Some(EntityTag::from_static("include-etag")),
        )
        .unwrap();
        let IncludeProperties::V2(include) = properties;
        let include = *include;

        assert_eq!(include.reference, reference);
        assert_eq!(include.name, "ZTEST");
        assert_eq!(include.object_type.to_string(), "PROG/I");
        assert_eq!(include.version, ObjectVersion::Active);
        assert_eq!(include.context_ref_count, 0);
        assert!(include.context_ref.is_none());
        assert_eq!(include.package.name(), "$TMP");
        assert_eq!(include.relations().len(), 7);
        assert_eq!(
            include.source.uri.as_str(),
            "/sap/bc/adt/programs/includes/ZTEST/source/main"
        );
        assert_eq!(include.source.etag.as_deref(), Some("202601241617490011"));
        assert_eq!(include.etag.as_deref(), Some("include-etag"));
    }

    #[test]
    fn parses_program_properties() {
        assert_program(&parse(PROGRAM_XML).unwrap());
    }

    #[test]
    fn rejects_malformed_program_xml() {
        let error = parse("<program:abapProgram>").unwrap_err();

        assert!(matches!(
            error,
            ResponseError::Object(ObjectError::InvalidResponse(_))
        ));
    }

    #[test]
    fn rejects_unsupported_program_object_version() {
        let body = PROGRAM_XML.replace("adtcore:version=\"inactive\"", "adtcore:version=\"dirty\"");
        let error = parse(&body).unwrap_err();

        assert!(matches!(
            error,
            ResponseError::Object(ObjectError::UnsupportedObjectVersion { version })
                if version == "dirty"
        ));
    }

    #[test]
    fn rejects_program_without_plain_text_source_link() {
        let body = PROGRAM_XML.replacen(
            "type=\"text/plain\"",
            "type=\"application/octet-stream\"",
            1,
        );
        let error = parse(&body).unwrap_err();

        assert!(matches!(
            error,
            ResponseError::Object(ObjectError::MissingRelation {
                relation: "plain-text source"
            })
        ));
    }

    #[test]
    fn rejects_disagreement_between_source_attribute_and_link() {
        let body = PROGRAM_XML.replace(
            "abapsource:sourceUri=\"source/main\"",
            "abapsource:sourceUri=\"source/other\"",
        );
        let error = parse(&body).unwrap_err();

        assert!(matches!(
            error,
            ResponseError::Object(ObjectError::RelationMismatch {
                relation: "source",
                declared,
                advertised,
            })
                if declared.ends_with("/source/other")
                    && advertised.ends_with("/source/main")
        ));
    }

    #[test]
    fn defers_invalid_optional_links_until_accessed() {
        let invalid_href = "https://attacker.example/sap/bc/adt/textelements/programs/Z_TEST";
        let body = PROGRAM_XML.replace("/sap/bc/adt/textelements/programs/z_test", invalid_href);

        let program = parse(&body).unwrap();
        let error = program.text_elements().unwrap_err();

        assert_eq!(error.href(), invalid_href);
    }

    #[test]
    fn retains_unknown_link_relations_and_representation_metadata() {
        let relations = Relations::new(
            ObjectRef::<Program>::for_test(
                "ZDEMO",
                crate::AdtUri::parse("/sap/bc/adt/programs/programs/ZDEMO").unwrap(),
            )
            .erase(),
            vec![AdvertisedLink {
                href: "related/resource?version=active#section".to_owned(),
                relation: Some("https://example.test/relations/future".to_owned()),
                media_type: Some("application/example+xml".to_owned()),
                hreflang: Some("en".to_owned()),
                title: Some("Future relation".to_owned()),
                length: Some("42".to_owned()),
                etag: Some("future-etag".to_owned()),
            }],
        );

        let link = relations.iter().next().unwrap().unwrap();
        assert_eq!(link.href, "related/resource?version=active#section");
        assert_eq!(
            link.target.as_str(),
            "/sap/bc/adt/programs/programs/ZDEMO/related/resource"
        );
        assert_eq!(link.query, [("version".to_owned(), "active".to_owned())]);
        assert_eq!(link.fragment.as_deref(), Some("section"));
        assert_eq!(
            link.relation.as_deref(),
            Some("https://example.test/relations/future")
        );
        assert_eq!(link.media_type.as_deref(), Some("application/example+xml"));
        assert_eq!(link.hreflang.as_deref(), Some("en"));
        assert_eq!(link.title.as_deref(), Some("Future relation"));
        assert_eq!(link.length.as_deref(), Some("42"));
        assert_eq!(link.etag.as_deref(), Some("future-etag"));
        let parser: Option<ParserRef> = relations.get().unwrap();
        assert!(parser.is_none());
    }
}
