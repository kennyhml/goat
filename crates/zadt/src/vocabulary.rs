/// Constants, types, headers and other vocabulary that does not
/// quite fit into any concrete components of the project at this time.
use http::HeaderName;

/// A stable category identity from an ADT discovery document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CategoryId {
    /// The category scheme URI.
    pub scheme: &'static str,

    /// The category term within the scheme.
    pub term: &'static str,
}

pub(crate) const PROGRAMS: CategoryId = CategoryId {
    scheme: "http://www.sap.com/adt/categories/programs",
    term: "programs",
};

pub(crate) const INCLUDES: CategoryId = CategoryId {
    scheme: "http://www.sap.com/adt/categories/programs",
    term: "includes",
};

pub(crate) const PROGRAM_RUN: CategoryId = CategoryId {
    scheme: "http://www.sap.com/adt/categories/programs",
    term: "programrun",
};

pub(crate) const PROGRAM_RUN_RELATION: &str =
    "http://www.sap.com/adt/relations/programs/programrun";

/// Relations currently understood in program representations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Relation {
    Versions,
    Source,
    ObjectStructure,
    TextElements,
    EnhancementImplementations,
    ObjectEnhancementOptions,
    SourceEnhancementOptions,
    ObjectStates,
    Parser,
}

pub const SECURITY_SESSION_HEADER: HeaderName = HeaderName::from_static("x-sap-security-session");
pub const PURPOSE_HEADER: HeaderName = HeaderName::from_static("sap-adt-purpose");
pub const LOAD_BALANCER_HEADER: HeaderName = HeaderName::from_static("sap-adt-saplb");
pub const CANCEL_ON_CLOSE_HEADER: HeaderName = HeaderName::from_static("sap-cancel-on-close");

impl Relation {
    pub fn from_uri(uri: &str) -> Option<Self> {
        match uri {
            "http://www.sap.com/adt/relations/versions" => Some(Self::Versions),
            "http://www.sap.com/adt/relations/source" => Some(Self::Source),
            "http://www.sap.com/adt/relations/objectstructure" => Some(Self::ObjectStructure),
            "http://www.sap.com/adt/relations/sources/textelements" => Some(Self::TextElements),
            "http://www.sap.com/adt/relations/enhancementImplementations" => {
                Some(Self::EnhancementImplementations)
            }
            "http://www.sap.com/adt/relations/enhancementOptionsOfMainObject" => {
                Some(Self::ObjectEnhancementOptions)
            }
            "http://www.sap.com/adt/relations/enhancementOptions" => {
                Some(Self::SourceEnhancementOptions)
            }
            "http://www.sap.com/adt/relations/objectstates" => Some(Self::ObjectStates),
            "http://www.sap.com/adt/relations/abapsource/parser" => Some(Self::Parser),
            _ => None,
        }
    }
}

/// Actions accepted through ADT's `_action` POST query parameter.
///
/// Values come from `IF_ADT_REST_POST_ACTION`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PostAction {
    Check,
    Activate,
    Lock,
    Unlock,
    Find,
}

impl PostAction {
    /// Returns the exact value expected by ADT.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Check => "CHECK",
            Self::Activate => "ACTIVATE",
            Self::Lock => "LOCK",
            Self::Unlock => "UNLOCK",
            Self::Find => "FIND",
        }
    }
}

pub(crate) mod query_parameter {
    pub const ACCESS_MODE: &str = "accessMode";
    pub const ACTION: &str = "_action";
    pub const LOCK_HANDLE: &str = "lockHandle";
    pub const PROFILER_ID: &str = "profilerId";
    pub const VERSION: &str = "version";
}

pub(crate) mod media_type {
    pub const DISCOVERY: &str = "application/atomsvc+xml";
    pub const HTML: &str = "text/html";
    pub const LOCK_RESULT: &str =
        "application/vnd.sap.as+xml; charset=utf-8; dataname=com.sap.adt.lock.Result2";
    pub const REPOSITORY_CONTENT_REQUEST: &str =
        "application/vnd.sap.adt.repository.virtualfolders.request.v1+xml";
    pub const REPOSITORY_OBJECT_PROPERTIES: &str =
        "application/vnd.sap.adt.repository.objproperties.result.v1+xml";
    pub const SOURCE: &str = "text/plain";
    pub const SOURCE_UPDATE: &str = "text/plain; charset=utf-8";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_actions_match_if_adt_rest_post_action() {
        assert_eq!(PostAction::Check.as_str(), "CHECK");
        assert_eq!(PostAction::Activate.as_str(), "ACTIVATE");
        assert_eq!(PostAction::Lock.as_str(), "LOCK");
        assert_eq!(PostAction::Unlock.as_str(), "UNLOCK");
        assert_eq!(PostAction::Find.as_str(), "FIND");
    }
}
