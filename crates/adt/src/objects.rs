use std::{borrow::Cow, fmt, hash::Hash, marker::PhantomData, str::FromStr};

use url::Url;

use crate::{
    client::{Client, Discovered},
    compatibility::CompatibilityError,
    error::ObjectError,
    uri::{AdtUri, AdtUriError},
    vocabulary::{CategoryId, INCLUDES, PROGRAMS},
};

mod policies;
mod properties;
mod version;

pub use policies::ObjectNamePolicy;
pub use properties::{ObjectProperties, ObjectPropertiesQuery};
pub use version::ObjectVersion;

pub(crate) mod private {
    pub trait Sealed {}
}

/// A global ABAP Workbench type consisting of an R3TR object-directory type and
/// an internal Workbench subtype.
///
/// # Background
///
/// A repository object generally has an entry in the object directory (`TADIR`)
/// with program ID `R3TR`. In contrast, `LIMU` identifies transportable
/// subobjects recorded in transport requests; those subobjects generally do not
/// have independent `TADIR` entries.
///
/// The R3TR object type identifies the owning repository object family, such as
/// `PROG`, `CLAS`, or `DDLS`. It does not by itself identify the particular
/// Workbench view or subobject.
///
/// Workbench subtypes are shorter internal identifiers defined by type pool
/// `SWBM` and registered in `WBOBJTYPES` and `WBOBJTYPT`. The `WBOBJTYPE`
/// structure combines the R3TR type in `OBJTYPE_TR` with the internal subtype in
/// `SUBTYPE_WB`. Workbench objects can map to transportable entities through
/// type-specific behavior that can be observed in `CL_WB_OBJECT`.
///
/// Much of this is an implementation detail. A global class has type `CLAS/OC`,
/// while one of its method implementations has type `CLAS/OM`. The method source
/// may be persisted in a generated include such as
/// `ZCL_DEMO_A_SET_TO_PAID========CM001` in `REPOSRC`. That generated program is
/// an include at the program-storage layer, but the method's Workbench subtype
/// remains `OM` it is not exposed as subtype `I`, nor does it gain a `TADIR` entry.
///
/// ADT serializes this pair with a slash, for example `PROG/P`, `PROG/I`, or
/// `CLAS/OC`. Values use their unpadded wire representation rather than the
/// trailing spaces of SAPs fixed-width `TROBJTYPE` and `SEU_OBJTYP` fields.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct GlobalWorkbenchType {
    directory_type: Cow<'static, str>,
    workbench_type: Cow<'static, str>,
}

impl GlobalWorkbenchType {
    /// Creates a global Workbench type from an R3TR object directory type and
    /// internal Workbench subtype.
    ///
    /// Both values must be ASCII. The directory type is limited to the four
    /// characters of `TROBJTYPE`, and the Workbench type to the three
    /// characters of `SEU_OBJTYP`.
    pub const fn new(directory_type: &'static str, workbench_type: &'static str) -> Self {
        assert!(directory_type.is_ascii(), "R3TR object type must be ASCII");
        assert!(
            directory_type.len() <= 4,
            "R3TR object type exceeds 4 characters"
        );
        assert!(workbench_type.is_ascii(), "Workbench type must be ASCII");
        assert!(
            workbench_type.len() <= 3,
            "Workbench type exceeds 3 characters"
        );
        Self {
            directory_type: Cow::Borrowed(directory_type),
            workbench_type: Cow::Borrowed(workbench_type),
        }
    }

    /// Returns the R3TR object type used in the object directory.
    pub fn directory_type(&self) -> &str {
        &self.directory_type
    }

    /// Returns the internal ABAP Workbench type.
    pub fn workbench_type(&self) -> &str {
        &self.workbench_type
    }
}

impl fmt::Display for GlobalWorkbenchType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.directory_type, self.workbench_type)
    }
}

/// An error parsing an ADT global Workbench type such as `PROG/I`.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("invalid global Workbench type `{value}`: {reason}")]
pub struct GlobalWorkbenchTypeParseError {
    value: String,
    reason: &'static str,
}

impl FromStr for GlobalWorkbenchType {
    type Err = GlobalWorkbenchTypeParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = |reason| GlobalWorkbenchTypeParseError {
            value: value.to_owned(),
            reason,
        };
        let (directory_type, workbench_type) = value
            .split_once('/')
            .ok_or_else(|| invalid("expected `<R3TR type>/<Workbench type>`"))?;
        if directory_type.is_empty() {
            return Err(invalid("R3TR object type is empty"));
        }
        if workbench_type.is_empty() {
            return Err(invalid("Workbench type is empty"));
        }
        if workbench_type.contains('/') {
            return Err(invalid("contains more than one separator"));
        }
        if !directory_type.is_ascii() {
            return Err(invalid("R3TR object type must be ASCII"));
        }
        if directory_type.len() > 4 {
            return Err(invalid("R3TR object type exceeds 4 characters"));
        }
        if !workbench_type.is_ascii() {
            return Err(invalid("Workbench type must be ASCII"));
        }
        if workbench_type.len() > 3 {
            return Err(invalid("Workbench type exceeds 3 characters"));
        }
        Ok(Self {
            directory_type: Cow::Owned(directory_type.to_owned()),
            workbench_type: Cow::Owned(workbench_type.to_owned()),
        })
    }
}

impl<'de> serde::Deserialize<'de> for GlobalWorkbenchType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

/// Statically identified ADT object resource family.
///
/// This allows object types to automatically implement various traits
/// and enables the objects capabilities to be located dynamically by
/// following the category in the discovery.
pub trait ObjectType: private::Sealed + Send + Sync + Sized + 'static {
    /// The category to identify the objects profile in a collection
    const CATEGORY: CategoryId;

    /// The objects global Workbench type.
    const TYPE: GlobalWorkbenchType;

    /// The objects naming constraints.
    const NAME_POLICY: ObjectNamePolicy;
}

/// A validated ADT object identity, optionally tagged with its static object type.
///
/// A bare `ObjectRef` is type-erased and proves only the objects identity and
/// location. [`Client::object`] returns `ObjectRef<T>` for a known [`ObjectType`].
pub struct ObjectRef<T = ()> {
    name: String,
    uri: AdtUri,
    marker: PhantomData<fn() -> T>,
}

impl ObjectRef {
    /// Creates a type-erased object reference from a validated ADT resource URI.
    pub fn new(uri: AdtUri) -> Self {
        Self {
            name: String::new(),
            uri,
            marker: PhantomData,
        }
    }

    /// Parses and validates a type-erased object resource URI.
    pub fn parse(value: &str) -> Result<Self, AdtUriError> {
        AdtUri::parse(value).map(Self::new)
    }
}

impl<T> ObjectRef<T> {
    fn typed(name: String, uri: AdtUri) -> Self {
        Self {
            name,
            uri,
            marker: PhantomData,
        }
    }

    /// Returns the object's resource URI.
    pub fn uri(&self) -> &AdtUri {
        &self.uri
    }

    /// Returns a type-erased copy of this object identity.
    pub fn erase(&self) -> ObjectRef {
        ObjectRef {
            name: self.name.clone(),
            uri: self.uri.clone(),
            marker: PhantomData,
        }
    }
}

impl<T: ObjectType> ObjectRef<T> {
    /// Returns the canonical uppercase object name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// The ABAP program object type.
#[derive(Debug)]
pub enum Program {}

impl ObjectType for Program {
    const CATEGORY: CategoryId = PROGRAMS;
    const TYPE: GlobalWorkbenchType = GlobalWorkbenchType::new("PROG", "P");
    const NAME_POLICY: ObjectNamePolicy = ObjectNamePolicy::new(30);
}

/// The standalone ABAP include object type.
#[derive(Debug)]
pub enum Include {}

impl ObjectType for Include {
    const CATEGORY: CategoryId = INCLUDES;
    const TYPE: GlobalWorkbenchType = GlobalWorkbenchType::new("PROG", "I");
    const NAME_POLICY: ObjectNamePolicy = ObjectNamePolicy::new(40);
}

/// A typed reference to an ABAP program.
pub type ProgramRef = ObjectRef<Program>;

/// A typed reference to a standalone ABAP include.
pub type IncludeRef = ObjectRef<Include>;

impl private::Sealed for Program {}
impl private::Sealed for Include {}

impl ObjectRef<Program> {
    #[cfg(test)]
    pub(crate) fn for_test(name: &str, uri: AdtUri) -> Self {
        Self::typed(name.to_ascii_uppercase(), uri)
    }
}

impl ObjectRef<Include> {
    #[cfg(test)]
    pub(crate) fn for_test(name: &str, uri: AdtUri) -> Self {
        Self::typed(name.to_ascii_uppercase(), uri)
    }
}

impl<T> Clone for ObjectRef<T> {
    fn clone(&self) -> Self {
        Self::typed(self.name.clone(), self.uri.clone())
    }
}

impl<T> fmt::Debug for ObjectRef<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("ObjectRef");
        if !self.name.is_empty() {
            debug.field("name", &self.name);
        }
        debug.field("uri", &self.uri).finish()
    }
}

impl<T> PartialEq for ObjectRef<T> {
    fn eq(&self, other: &Self) -> bool {
        self.uri == other.uri
    }
}

impl<T> Eq for ObjectRef<T> {}

impl<T> Hash for ObjectRef<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.uri.hash(state);
    }
}

impl<T> fmt::Display for ObjectRef<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.uri.fmt(formatter)
    }
}

impl From<AdtUri> for ObjectRef {
    fn from(value: AdtUri) -> Self {
        Self::new(value)
    }
}

impl Client<Discovered> {
    /// Resolves a typed object reference from its statically known collection.
    ///
    /// Constructing a reference performs no request; the collection URI comes
    /// from the capabilities already retained by the discovered client.
    pub fn object<T: ObjectType>(&self, name: &str) -> Result<ObjectRef<T>, ObjectError> {
        T::NAME_POLICY.validate(name)?;
        let name = name.to_ascii_uppercase();
        let uri_name = name.to_ascii_lowercase();
        let collection = self
            .collection(T::CATEGORY)
            .ok_or(CompatibilityError::MissingCollection(T::CATEGORY))?;
        let uri = append_segments(collection.target(), [&uri_name])?;
        Ok(ObjectRef::typed(name, uri))
    }
}

pub(crate) fn append_segments<I, S>(base: &AdtUri, segments: I) -> Result<AdtUri, AdtUriError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
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
    fn global_workbench_types_use_unpadded_sap_field_limits() {
        let object_type = GlobalWorkbenchType::new("ABCD", "XYZ");

        assert_eq!(object_type.directory_type(), "ABCD");
        assert_eq!(object_type.workbench_type(), "XYZ");
        assert_eq!(object_type.to_string(), "ABCD/XYZ");
        assert_eq!(Program::TYPE.to_string(), "PROG/P");
        assert_eq!(Include::TYPE.to_string(), "PROG/I");
    }

    #[test]
    fn parses_an_owned_global_workbench_type() {
        let object_type: GlobalWorkbenchType = "CLAS/OM".parse().unwrap();

        assert_eq!(object_type.directory_type(), "CLAS");
        assert_eq!(object_type.workbench_type(), "OM");
        assert_eq!(object_type.to_string(), "CLAS/OM");
    }

    #[test]
    fn rejects_invalid_global_workbench_type_responses() {
        for value in [
            "CLAS",
            "/OM",
            "CLAS/",
            "CLAS/OM/X",
            "TOOLONG/X",
            "CLAS/LONG",
        ] {
            assert!(
                value.parse::<GlobalWorkbenchType>().is_err(),
                "accepted {value}"
            );
        }
    }

    #[test]
    #[should_panic(expected = "R3TR object type exceeds 4 characters")]
    fn global_workbench_type_rejects_an_oversized_directory_type() {
        GlobalWorkbenchType::new("ABCDE", "X");
    }

    #[test]
    #[should_panic(expected = "Workbench type exceeds 3 characters")]
    fn global_workbench_type_rejects_an_oversized_internal_type() {
        GlobalWorkbenchType::new("ABCD", "WXYZ");
    }

    #[test]
    fn object_name_policies_enforce_type_specific_limits() {
        assert_eq!(Program::NAME_POLICY.maximum_length(), 30);
        assert_eq!(Include::NAME_POLICY.maximum_length(), 40);
        assert!(Program::NAME_POLICY.validate(&"A".repeat(30)).is_ok());
        assert!(Include::NAME_POLICY.validate(&"A".repeat(40)).is_ok());

        let name = "A".repeat(31);
        let error = Program::NAME_POLICY.validate(&name).unwrap_err();
        assert!(matches!(
            error,
            ObjectError::NameTooLong {
                name: rejected,
                maximum_length: 30,
            } if rejected == name
        ));
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
}
