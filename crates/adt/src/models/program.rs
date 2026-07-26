use serde::Deserialize;

use crate::{
    AdtLink, EnhancementImplementationsRef, EnhancementOptionsRef, HtmlSourceRef, ObjectRef,
    ObjectStateRef, ObjectStructureRef, ObjectVersion, PackageRef, ParserRef, ProgramError,
    ProgramRef, SourceRef, SourceVersionsRef, TextElementsRef,
    resource::{AdtLinkMetadata, resolve_href},
    vocabulary::{Relation, media_type},
};

pub(crate) fn parse_program(
    reference: ProgramRef,
    body: &[u8],
    etag: Option<String>,
) -> Result<Program, ProgramError> {
    let raw: RawProgram = serde_xml_rs::from_reader(body).map_err(ProgramError::InvalidResponse)?;
    Program::from_raw(reference, raw, etag)
}

/// A fetched ABAP program descriptor normalized from V2 or V3 XML.
#[derive(Clone, Debug)]
#[readonly::make]
pub struct Program {
    /// The program resource that was fetched.
    pub reference: ProgramRef,

    /// The program name supplied by SAP.
    pub name: String,

    /// The repository object type, normally `PROG/P`.
    pub object_type: String,

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
    pub package: PackageRef,

    /// The syntax configuration and parser advertised for the source.
    pub syntax_configuration: SyntaxConfiguration,

    /// The advertised plain-text source representation.
    pub source: SourceRef,

    /// The advertised rendered HTML source representation.
    pub html_source: Option<HtmlSourceRef>,

    /// The source version-history resource.
    pub versions: Option<SourceVersionsRef>,

    /// The program's object-structure resource.
    pub object_structure: Option<ObjectStructureRef>,

    /// The program's text-elements resource.
    pub text_elements: Option<TextElementsRef>,

    /// Enhancement implementations associated with the program.
    pub enhancement_implementations: Option<EnhancementImplementationsRef>,

    /// Enhancement options associated with the program object.
    pub enhancement_options: Option<EnhancementOptionsRef>,

    /// Enhancement options associated with the program source.
    pub source_enhancement_options: Option<EnhancementOptionsRef>,

    /// A link to the program's other active or inactive state.
    pub object_state: Option<ObjectStateRef>,

    /// All top-level links advertised by the program representation.
    pub links: Vec<AdtLink>,

    /// The entity tag of this program descriptor, when present.
    pub etag: Option<String>,
}

impl Program {
    fn from_raw(
        reference: ProgramRef,
        raw: RawProgram,
        etag: Option<String>,
    ) -> Result<Self, ProgramError> {
        let package_object = ObjectRef::parse(&raw.package.uri).map_err(|source| {
            ProgramError::InvalidPackageUri {
                uri: raw.package.uri.clone(),
                source,
            }
        })?;
        let package = PackageRef::new(raw.package.name, raw.package.object_type, package_object);
        let version = ObjectVersion::parse(&raw.version).ok_or_else(|| {
            ProgramError::UnsupportedObjectVersion {
                version: raw.version.clone(),
            }
        })?;
        let links = resolve_links(reference.uri(), raw.links)?;
        let source_link = find_link(&links, Relation::Source, Some(media_type::SOURCE))
            .ok_or(ProgramError::MissingSourceLink)?;
        let source = SourceRef::from_link(reference.object().clone(), source_link);
        let declared_source = resolve_href(reference.uri(), &raw.source_uri).map_err(|source| {
            ProgramError::InvalidLink {
                href: raw.source_uri.clone(),
                source,
            }
        })?;
        if declared_source.target != source.uri {
            return Err(ProgramError::SourceLinkMismatch {
                declared: declared_source.target.to_string(),
                advertised: source.uri.to_string(),
            });
        }

        let html_source = find_link(&links, Relation::Source, Some(media_type::HTML))
            .map(HtmlSourceRef::from_link);
        let versions = typed_link::<SourceVersionsRef>(&links, Relation::Versions);
        let object_structure = typed_link::<ObjectStructureRef>(&links, Relation::ObjectStructure);
        let text_elements = typed_link::<TextElementsRef>(&links, Relation::TextElements);
        let enhancement_implementations = typed_link::<EnhancementImplementationsRef>(
            &links,
            Relation::EnhancementImplementations,
        );
        let enhancement_options =
            typed_link::<EnhancementOptionsRef>(&links, Relation::ObjectEnhancementOptions);
        let source_enhancement_options =
            typed_link::<EnhancementOptionsRef>(&links, Relation::SourceEnhancementOptions);
        let object_state = typed_link::<ObjectStateRef>(&links, Relation::ObjectStates);

        let syntax_links = resolve_links(reference.uri(), raw.syntax_configuration.language.links)?;
        let parser = typed_link::<ParserRef>(&syntax_links, Relation::Parser);

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
                    parser,
                    links: syntax_links,
                },
            },
            source,
            html_source,
            versions,
            object_structure,
            text_elements,
            enhancement_implementations,
            enhancement_options,
            source_enhancement_options,
            object_state,
            links,
            etag,
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

    /// The parser grammar advertised for this language.
    pub parser: Option<ParserRef>,

    /// All links advertised for this language configuration.
    pub links: Vec<AdtLink>,
}

trait FromAdtLink: Sized {
    fn from_adt_link(link: &AdtLink) -> Self;
}

macro_rules! resolved_link_conversion {
    ($($name:ident),+ $(,)?) => {
        $(
            impl FromAdtLink for $name {
                fn from_adt_link(link: &AdtLink) -> Self {
                    Self::from_link(link)
                }
            }
        )+
    };
}

resolved_link_conversion!(
    SourceVersionsRef,
    ObjectStructureRef,
    TextElementsRef,
    EnhancementImplementationsRef,
    EnhancementOptionsRef,
    ObjectStateRef,
    ParserRef,
);

fn typed_link<T: FromAdtLink>(links: &[AdtLink], relation: Relation) -> Option<T> {
    find_link(links, relation, None).map(T::from_adt_link)
}

fn find_link<'a>(
    links: &'a [AdtLink],
    relation: Relation,
    media_type: Option<&str>,
) -> Option<&'a AdtLink> {
    links.iter().find(|link| {
        link.relation.as_deref().and_then(Relation::from_uri) == Some(relation)
            && media_type.is_none_or(|expected| {
                link.media_type.as_deref().is_some_and(|actual| {
                    actual
                        .split(';')
                        .next()
                        .is_some_and(|value| value.trim().eq_ignore_ascii_case(expected))
                })
            })
    })
}

fn resolve_links(
    base: &crate::AdtUri,
    links: Vec<RawAtomLink>,
) -> Result<Vec<AdtLink>, ProgramError> {
    links
        .into_iter()
        .map(|link| {
            let href = link.href.clone();
            AdtLink::from_href(
                base,
                link.href,
                AdtLinkMetadata {
                    relation: link.relation,
                    media_type: link.media_type,
                    hreflang: link.hreflang,
                    title: link.title,
                    length: link.length,
                    etag: link.etag,
                },
            )
            .map_err(|source| ProgramError::InvalidLink { href, source })
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(rename = "program:abapProgram")]
struct RawProgram {
    #[serde(rename = "@adtcore:name")]
    name: String,
    #[serde(rename = "@adtcore:type")]
    object_type: String,
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
    links: Vec<RawAtomLink>,
}

#[derive(Deserialize)]
struct RawPackage {
    #[serde(rename = "@adtcore:name")]
    name: String,
    #[serde(rename = "@adtcore:uri")]
    uri: String,
    #[serde(rename = "@adtcore:type")]
    object_type: String,
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
    links: Vec<RawAtomLink>,
}

#[derive(Deserialize)]
struct RawAtomLink {
    #[serde(rename = "@href")]
    href: String,
    #[serde(rename = "@rel")]
    relation: Option<String>,
    #[serde(rename = "@type")]
    media_type: Option<String>,
    #[serde(rename = "@hreflang")]
    hreflang: Option<String>,
    #[serde(rename = "@title")]
    title: Option<String>,
    #[serde(rename = "@length")]
    length: Option<String>,
    #[serde(rename = "@etag")]
    etag: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROGRAM_XML: &str = include_str!("../../tests/fixtures/program-z-test.xml");

    fn parse(body: &str) -> Result<Program, ProgramError> {
        parse_program(
            ProgramRef::for_test(
                crate::AdtUri::parse("/sap/bc/adt/programs/programs/Z_TEST").unwrap(),
            ),
            body.as_bytes(),
            Some("program-etag".to_owned()),
        )
    }

    fn assert_program(program: &Program) {
        assert_eq!(program.name, "Z_TEST");
        assert_eq!(program.version, ObjectVersion::Inactive);
        assert_eq!(program.etag.as_deref(), Some("program-etag"));
        assert_eq!(
            program.source.uri.as_str(),
            "/sap/bc/adt/programs/programs/Z_TEST/source/main"
        );
        assert_eq!(program.source.etag.as_deref(), Some("202607251959580001"));
        assert_eq!(program.links.len(), 9);
        assert_eq!(program.syntax_configuration.language.links.len(), 1);
        assert_eq!(
            program
                .syntax_configuration
                .language
                .parser
                .as_ref()
                .unwrap()
                .etag
                .as_deref(),
            Some("757")
        );
    }

    #[test]
    fn parses_program_response() {
        assert_program(&parse(PROGRAM_XML).unwrap());
    }

    #[test]
    fn rejects_malformed_program_xml() {
        let error = parse("<program:abapProgram>").unwrap_err();

        assert!(matches!(error, ProgramError::InvalidResponse(_)));
    }

    #[test]
    fn rejects_unsupported_program_object_version() {
        let body = PROGRAM_XML.replace("adtcore:version=\"inactive\"", "adtcore:version=\"dirty\"");
        let error = parse(&body).unwrap_err();

        assert!(matches!(
            error,
            ProgramError::UnsupportedObjectVersion { version } if version == "dirty"
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

        assert!(matches!(error, ProgramError::MissingSourceLink));
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
            ProgramError::SourceLinkMismatch { declared, advertised }
                if declared.ends_with("/source/other")
                    && advertised.ends_with("/source/main")
        ));
    }

    #[test]
    fn retains_unknown_link_relations_and_representation_metadata() {
        let base = crate::AdtUri::parse("/sap/bc/adt/programs/programs/ZDEMO").unwrap();
        let links = resolve_links(
            &base,
            vec![RawAtomLink {
                href: "related/resource?version=active#section".to_owned(),
                relation: Some("https://example.test/relations/future".to_owned()),
                media_type: Some("application/example+xml".to_owned()),
                hreflang: Some("en".to_owned()),
                title: Some("Future relation".to_owned()),
                length: Some("42".to_owned()),
                etag: Some("future-etag".to_owned()),
            }],
        )
        .unwrap();

        let link = &links[0];
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
        assert!(typed_link::<ParserRef>(&links, Relation::Parser).is_none());
    }
}
