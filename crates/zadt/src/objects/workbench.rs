use core::fmt;
use std::{borrow::Cow, str::FromStr};

/// A global ABAP Workbench type consisting of an R3TR object-directory type and
/// an internal Workbench subtype.
///
/// # Background
///
/// A repository object generally has an entry in the object directory (`TADIR`)
/// with program ID `R3TR`. In contrast, `LIMU` identifies transportable
/// subobjects recorded in transport requests - those subobjects generally do not
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
/// may be persisted in a generated include such as `ZCL_DEMO_A_SET_TO_PAID========CM001`
/// in `REPOSRC`. That generated program is an include at the program-storage layer,
/// but the method's Workbench subtype remains `OM` it is not exposed as subtype `I`,
/// nor does it gain a `TADIR` entry.
///
/// ADT serializes this pair with a slash, for example `PROG/P`, `PROG/I`, or
/// `CLAS/OC`. Values use their unpadded wire representation rather than the
/// trailing spaces of SAPs fixed-width `TROBJTYPE` and `SEU_OBJTYP` fields.
#[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Deserialize)]
#[serde(try_from = "String")]
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
pub struct InvalidWorkbenchType {
    value: String,
    reason: &'static str,
}

impl FromStr for GlobalWorkbenchType {
    type Err = InvalidWorkbenchType;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = |reason| InvalidWorkbenchType {
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

impl TryFrom<String> for GlobalWorkbenchType {
    type Error = InvalidWorkbenchType;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Include, ObjectType, Program};

    #[test]
    fn global_workbench_types_use_unpadded_sap_field_limits() {
        let object_type = GlobalWorkbenchType::new("ABCD", "XYZ");

        assert_eq!(object_type.directory_type(), "ABCD");
        assert_eq!(object_type.workbench_type(), "XYZ");
        assert_eq!(object_type.to_string(), "ABCD/XYZ");
        assert_eq!(Program::WORKBENCH_TYPE.to_string(), "PROG/P");
        assert_eq!(Include::WORKBENCH_TYPE.to_string(), "PROG/I");
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
}
