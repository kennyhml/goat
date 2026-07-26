use std::fmt;

use url::Url;

use crate::{
    AdtUri, AdtUriError, Capabilities, CategoryId, IncludeError, ProgramError,
    vocabulary::{INCLUDES, PROGRAMS},
};
const LINK_RESOLUTION_ORIGIN: &str = "https://adt.invalid";

/// A concrete link advertised by an ADT resource representation.
///
/// The original Atom metadata is retained alongside a resolved, validated
/// target. This lets callers use known relations through typed resource
/// references without discarding unknown relations or representation hints.
///
/// Handled by `IF_ATOM_TYPES=>link_s` on the backend.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[readonly::make]
pub struct AdtLink {
    /// The link exactly as advertised by SAP.
    pub href: String,

    /// The validated resource path produced by resolving `href`.
    pub target: AdtUri,

    /// Decoded query parameters in their advertised order.
    pub query: Vec<(String, String)>,

    /// The optional link fragment, without the leading `#`.
    pub fragment: Option<String>,

    /// The Atom relation identifying what the target means to its source.
    pub relation: Option<String>,

    /// The media type of the target representation, when advertised.
    pub media_type: Option<String>,

    /// The language of the target representation, when advertised.
    pub hreflang: Option<String>,

    /// A human-readable label for the link.
    pub title: Option<String>,

    /// The advertised target length.
    pub length: Option<String>,

    /// SAP's entity-tag extension for the target representation.
    pub etag: Option<String>,
}

impl AdtLink {
    pub(crate) fn from_href(
        base: &AdtUri,
        href: String,
        metadata: AdtLinkMetadata,
    ) -> Result<Self, AdtUriError> {
        let resolved = resolve_href(base, &href)?;
        Ok(Self {
            href,
            target: resolved.target,
            query: resolved.query,
            fragment: resolved.fragment,
            relation: metadata.relation,
            media_type: metadata.media_type,
            hreflang: metadata.hreflang,
            title: metadata.title,
            length: metadata.length,
            etag: metadata.etag,
        })
    }
}

pub(crate) struct AdtLinkMetadata {
    pub relation: Option<String>,
    pub media_type: Option<String>,
    pub hreflang: Option<String>,
    pub title: Option<String>,
    pub length: Option<String>,
    pub etag: Option<String>,
}

pub(crate) struct ResolvedHref {
    pub target: AdtUri,
    pub query: Vec<(String, String)>,
    pub fragment: Option<String>,
}

/// Resolves an href without assigning Atom link semantics to it.
pub(crate) fn resolve_href(base: &AdtUri, href: &str) -> Result<ResolvedHref, AdtUriError> {
    if href.is_empty() {
        return Err(AdtUriError::Empty);
    }
    if href.trim() != href || href.chars().any(char::is_control) || href.contains('\\') {
        return Err(AdtUriError::InvalidCharacters);
    }
    if href.starts_with("//") || Url::parse(href).is_ok() {
        return Err(AdtUriError::Absolute);
    }

    let base_url = Url::parse(&format!("{LINK_RESOLUTION_ORIGIN}{base}"))?;
    let resolved = if href.starts_with('/')
        || href.starts_with("./")
        || href.starts_with("../")
        || href.starts_with('?')
        || href.starts_with('#')
    {
        base_url.join(href)?
    } else {
        let mut directory = base_url;
        directory
            .path_segments_mut()
            .expect("an HTTP URL supports path segments")
            .push("");
        directory.join(href)?
    };

    Ok(ResolvedHref {
        target: AdtUri::parse(resolved.path())?,
        query: resolved
            .query_pairs()
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect(),
        fragment: resolved.fragment().map(str::to_owned),
    })
}

impl fmt::Display for AdtLink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.href)
    }
}

/// An ADT repository-object version accepted by the `version` query parameter.
///
/// These values are the public URI vocabulary from `IF_ADT_URI_QUERY_PARAMETERS`.
/// SAP maps them internally to one-character ABAP Workbench states.
///
/// # SAP references
///
/// - `IF_ADT_URI_QUERY_PARAMETERS` defines `CO_VERSION` and all external values;
/// - `CL_SEDI_ADT_RES_SOURCE->GET` reads the query parameter for programs;
/// - `CL_ADT_UTILITY->GET_WB_VERSION` maps it to Workbench `R3STATE` values.
///
/// Some constants can also be found in the type-pool `SWBM`
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ObjectVersion {
    /// The persistent active object (R3STATE `A`)
    Active,

    /// An inactive object awaiting activation (R3STATE `I`)
    Inactive,

    /// Uses the inactive version if the requesting user is editing it,
    /// otherwise the active version (R3STATE `_` - may not exist)
    WorkingArea,

    /// A newly created object (R3STATE `N`)
    New,

    /// An object for which only part of the content is active (R3STATE `P`)
    PartlyActive,
}

impl ObjectVersion {
    /// Returns the exact value used by ADT URI query parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::WorkingArea => "workingArea",
            Self::New => "new",
            Self::PartlyActive => "partlyActive",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "inactive" => Some(Self::Inactive),
            "workingArea" => Some(Self::WorkingArea),
            "new" => Some(Self::New),
            "partlyActive" => Some(Self::PartlyActive),
            _ => None,
        }
    }
}

impl fmt::Display for ObjectVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

macro_rules! relation_ref {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, PartialEq)]
        #[readonly::make]
        pub struct $name {
            /// The validated related-resource URI.
            pub uri: AdtUri,

            /// Query parameters advertised as part of the relation.
            pub query: Vec<(String, String)>,

            /// The optional link fragment, without the leading `#`.
            pub fragment: Option<String>,

            /// The entity tag advertised for this resource, when present.
            pub etag: Option<String>,
        }

        impl $name {
            pub(crate) fn from_link(link: &AdtLink) -> Self {
                Self {
                    uri: link.target.clone(),
                    query: link.query.clone(),
                    fragment: link.fragment.clone(),
                    etag: link.etag.clone(),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.uri.fmt(formatter)
            }
        }
    };
}

relation_ref!(
    /// The rendered HTML representation of an object's source.
    HtmlSourceRef
);
relation_ref!(
    /// The version history advertised for a source resource.
    SourceVersionsRef
);
relation_ref!(
    /// The structural representation advertised for an ADT object.
    ObjectStructureRef
);
relation_ref!(
    /// The text-elements resource advertised for an ADT object.
    TextElementsRef
);
relation_ref!(
    /// The enhancement implementations associated with an ADT object.
    EnhancementImplementationsRef
);
relation_ref!(
    /// The enhancement options associated with an ADT object or source.
    EnhancementOptionsRef
);
relation_ref!(
    /// A link to another state, such as the active version, of an ADT object.
    ObjectStateRef
);
relation_ref!(
    /// The parser grammar advertised by an ABAP syntax configuration.
    ParserRef
);

/// A validated reference to an ADT repository object.
///
/// An object reference is an identity and resource location, not a fetched
/// object representation or proof of protocol capabilities. In particular, it
/// does not imply that the object has source code or supports locking.
/// Constructing one performs no I/O.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ObjectRef(AdtUri);

impl ObjectRef {
    /// Creates an object reference from a validated ADT resource URI.
    pub fn new(uri: AdtUri) -> Self {
        Self(uri)
    }

    /// Parses and validates an object resource URI.
    pub fn parse(value: &str) -> Result<Self, AdtUriError> {
        AdtUri::parse(value).map(Self)
    }

    /// Returns the object's resource URI.
    pub fn uri(&self) -> &AdtUri {
        &self.0
    }
}

impl fmt::Display for ObjectRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl From<AdtUri> for ObjectRef {
    fn from(value: AdtUri) -> Self {
        Self::new(value)
    }
}

/// A package reference embedded in an ADT object representation.
#[derive(Clone, Debug, Eq, PartialEq)]
#[readonly::make]
pub struct PackageRef {
    /// The package name.
    pub name: String,

    /// The repository object type, normally `DEVC/K`.
    pub object_type: String,

    /// The package's validated object reference.
    pub object: ObjectRef,
}

impl PackageRef {
    pub(crate) fn new(name: String, object_type: String, object: ObjectRef) -> Self {
        Self {
            name,
            object_type,
            object,
        }
    }
}

/// A validated source-code resource and its owning repository object.
///
/// A source URI alone does not establish which object lock authorizes an
/// update. `SourceRef` therefore retains both the source URI and its
/// [`ObjectRef`]. [`SourceRef::update`](crate::SourceRef::update) uses that
/// relationship to validate a [`LockHandle`](crate::LockHandle) at build time.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[readonly::make]
pub struct SourceRef {
    /// The object that owns this source resource.
    pub object: ObjectRef,

    /// The source resource URI.
    pub uri: AdtUri,

    /// Query parameters advertised as part of the source link.
    pub query: Vec<(String, String)>,

    /// The optional source fragment, without the leading `#`.
    pub fragment: Option<String>,

    /// The entity tag advertised for this source, when present.
    pub etag: Option<String>,
}

impl fmt::Display for SourceRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.uri.fmt(formatter)
    }
}

/// Constructs a typed reference from central-discovery capabilities.
///
/// Implementations identify their collection through [`FromDiscovery::CATEGORY`]
/// and apply the domain-specific rules for resolving a named member. The
/// conversion performs no request.
pub trait FromDiscovery: Sized {
    /// The error produced while resolving this reference.
    type Error;

    /// The stable discovery category identifying the reference's collection.
    const CATEGORY: CategoryId;

    /// Resolves `name` using the supplied central-discovery capabilities.
    fn from_discovery(capabilities: &Capabilities, name: &str) -> Result<Self, Self::Error>;
}

/// A program identity resolved from the programs collection in central discovery.
///
/// [`Client::object`](crate::Client::object) does not require callers to know the
/// programs collection URI. It looks up the stable programs category and
/// appends the program name as one encoded path segment.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ProgramRef(ObjectRef);

impl ProgramRef {
    #[cfg(test)]
    pub(crate) fn for_test(uri: AdtUri) -> Self {
        Self(ObjectRef::new(uri))
    }

    /// Returns the program object reference.
    pub fn object(&self) -> &ObjectRef {
        &self.0
    }

    /// Returns the program object URI.
    pub fn uri(&self) -> &AdtUri {
        self.0.uri()
    }

    /// Returns the program's conventional `source/main` resource.
    ///
    /// This convention belongs to the ADT program resource profile; it is not
    /// implied by the underlying [`ObjectRef`]. A fetched [`Program`](crate::Program)
    /// instead exposes the source link advertised by SAP.
    pub fn source(&self) -> SourceRef {
        let uri = append_segments(self.uri(), ["source", "main"])
            .expect("static program source path segments form a valid ADT URI");
        SourceRef {
            object: self.0.clone(),
            uri,
            query: Vec::new(),
            fragment: None,
            etag: None,
        }
    }
}

impl fmt::Display for ProgramRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromDiscovery for ProgramRef {
    type Error = ProgramError;

    const CATEGORY: CategoryId = PROGRAMS;

    fn from_discovery(
        capabilities: &Capabilities,
        program_name: &str,
    ) -> Result<Self, Self::Error> {
        validate_program_name(program_name)?;
        let collection = capabilities
            .collection(Self::CATEGORY.scheme, Self::CATEGORY.term)
            .ok_or(ProgramError::MissingCollection)?;
        let uri = append_segments(collection.target(), [program_name])?;
        Ok(Self(ObjectRef::new(uri)))
    }
}

/// A standalone ABAP include resolved from the includes collection.
///
/// Includes share the ADT programs domain with [`ProgramRef`], but use their
/// own discovery collection and representation contract.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct IncludeRef(ObjectRef);

impl IncludeRef {
    #[cfg(test)]
    pub(crate) fn for_test(uri: AdtUri) -> Self {
        Self(ObjectRef::new(uri))
    }

    /// Returns the include object reference.
    pub fn object(&self) -> &ObjectRef {
        &self.0
    }

    /// Returns the include object URI.
    pub fn uri(&self) -> &AdtUri {
        self.0.uri()
    }

    /// Returns the include's conventional `source/main` resource.
    pub fn source(&self) -> SourceRef {
        let uri = append_segments(self.uri(), ["source", "main"])
            .expect("static include source path segments form a valid ADT URI");
        SourceRef {
            object: self.0.clone(),
            uri,
            query: Vec::new(),
            fragment: None,
            etag: None,
        }
    }
}

impl fmt::Display for IncludeRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromDiscovery for IncludeRef {
    type Error = IncludeError;

    const CATEGORY: CategoryId = INCLUDES;

    fn from_discovery(
        capabilities: &Capabilities,
        include_name: &str,
    ) -> Result<Self, Self::Error> {
        validate_include_name(include_name)?;
        let collection = capabilities
            .collection(Self::CATEGORY.scheme, Self::CATEGORY.term)
            .ok_or(IncludeError::MissingCollection)?;
        let uri = append_segments(collection.target(), [include_name])?;
        Ok(Self(ObjectRef::new(uri)))
    }
}

impl SourceRef {
    pub(crate) fn from_link(object: ObjectRef, link: &AdtLink) -> Self {
        Self {
            object,
            uri: link.target.clone(),
            query: link.query.clone(),
            fragment: link.fragment.clone(),
            etag: link.etag.clone(),
        }
    }
}

fn validate_program_name(program_name: &str) -> Result<(), ProgramError> {
    if program_name.is_empty()
        || program_name.trim() != program_name
        || program_name.chars().any(char::is_control)
        || matches!(program_name, "." | "..")
    {
        return Err(ProgramError::InvalidName {
            name: program_name.to_owned(),
        });
    }
    Ok(())
}

fn validate_include_name(include_name: &str) -> Result<(), IncludeError> {
    if include_name.is_empty()
        || include_name.trim() != include_name
        || include_name.chars().any(char::is_control)
        || matches!(include_name, "." | "..")
    {
        return Err(IncludeError::InvalidName {
            name: include_name.to_owned(),
        });
    }
    Ok(())
}

fn append_segments<I, T>(base: &AdtUri, segments: I) -> Result<AdtUri, AdtUriError>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let mut url = Url::parse(&format!("https://adt.invalid{}", base.as_str()))
        .expect("a validated root-relative ADT URI forms a valid URL");
    url.path_segments_mut()
        .expect("an HTTP URL supports path segments")
        .extend(
            segments
                .into_iter()
                .map(|segment| segment.as_ref().to_owned()),
        );
    AdtUri::parse(url.path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_the_conventional_source_from_a_program_reference() {
        let program =
            ProgramRef::for_test(AdtUri::parse("/sap/bc/adt/programs/programs/ZPROGRAM").unwrap());

        assert_eq!(
            program.source().uri.as_str(),
            "/sap/bc/adt/programs/programs/ZPROGRAM/source/main"
        );
    }

    #[test]
    fn encodes_dynamic_names_as_single_path_segments() {
        let collection = AdtUri::parse("/sap/bc/adt/programs/programs").unwrap();

        assert_eq!(
            append_segments(&collection, ["/DMO/PROGRAM"])
                .unwrap()
                .as_str(),
            "/sap/bc/adt/programs/programs/%2FDMO%2FPROGRAM"
        );
    }

    #[test]
    fn resolves_the_relative_link_forms_emitted_by_programs() {
        let program = AdtUri::parse("/sap/bc/adt/programs/programs/ZDEMO").unwrap();

        let source = resolve_href(&program, "source/main?version=active").unwrap();
        assert_eq!(
            source.target.as_str(),
            "/sap/bc/adt/programs/programs/ZDEMO/source/main"
        );
        assert_eq!(source.query, [("version".to_owned(), "active".to_owned())]);

        let structure = resolve_href(&program, "./ZDEMO/objectstructure?version=inactive").unwrap();
        assert_eq!(
            structure.target.as_str(),
            "/sap/bc/adt/programs/programs/ZDEMO/objectstructure"
        );
        assert_eq!(
            structure.query,
            [("version".to_owned(), "inactive".to_owned())]
        );

        let root_relative = resolve_href(
            &program,
            "/sap/bc/adt/textelements/programs/ZDEMO#selectionTexts",
        )
        .unwrap();
        assert_eq!(
            root_relative.target.as_str(),
            "/sap/bc/adt/textelements/programs/ZDEMO"
        );
        assert_eq!(root_relative.fragment.as_deref(), Some("selectionTexts"));
    }

    #[test]
    fn rejects_links_outside_the_sap_resource_namespace() {
        let program = AdtUri::parse("/sap/bc/adt/programs/programs/ZDEMO").unwrap();

        for href in [
            "https://attacker.example/sap/bc/adt/programs/ZDEMO",
            "//attacker.example/sap/bc/adt/programs/ZDEMO",
            "/sap/public/bc/icf/logoff",
            "../../../../../public/bc/icf/logoff",
        ] {
            assert!(resolve_href(&program, href).is_err(), "accepted {href}");
        }
    }

    #[test]
    fn object_versions_use_the_adt_query_parameter_vocabulary() {
        for (version, value) in [
            (ObjectVersion::Active, "active"),
            (ObjectVersion::Inactive, "inactive"),
            (ObjectVersion::WorkingArea, "workingArea"),
            (ObjectVersion::New, "new"),
            (ObjectVersion::PartlyActive, "partlyActive"),
        ] {
            assert_eq!(version.as_str(), value);
            assert_eq!(ObjectVersion::parse(value), Some(version));
        }
    }
}
