/// Version of a repository object.
///
/// These values are the public URI vocabulary for `IF_ADT_URI_QUERY_PARAMETERS`.
/// SAP maps them internally to one-character ABAP Workbench `R3STATE` values.
///
/// # References
/// - `IF_ADT_URI_QUERY_PARAMETERS` defines `CO_VERSION` and its external values
/// - `CL_ADT_UTILITY->GET_WB_VERSION` maps it to Workbench `R3STATE` values
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ObjectVersion {
    /// The persistent active object (R3STATE `A`).
    Active,

    /// An inactive object awaiting activation (R3STATE `I`).
    Inactive,

    /// Uses the current users inactive version when available (R3STATE `_`).
    WorkingArea,

    /// A newly created object (R3STATE `N`).
    New,

    /// An object for which only part of the content is active (R3STATE `P`).
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

impl std::fmt::Display for ObjectVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_the_adt_query_parameter_vocabulary() {
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
