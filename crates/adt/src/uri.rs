use std::fmt;

use thiserror::Error;
use url::Url;

/// The conventional root beneath which relative ADT resource paths resolve.
pub const ADT_ROOT: &str = "/sap/bc/adt";

/// The SAP HTTP namespace containing ADT and ADT-advertised companion resources.
pub const ADT_RESOURCE_ROOT: &str = "/sap/bc";
const VALIDATION_ORIGIN: &str = "https://adt.invalid";

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AdtUriError {
    #[error("ADT resource URI cannot be empty")]
    Empty,

    #[error("absolute and authority URLs are not valid ADT resource URIs")]
    Absolute,

    #[error("ADT resource URI contains invalid characters")]
    InvalidCharacters,

    #[error("ADT resource URI must remain below {ADT_RESOURCE_ROOT}")]
    OutsideRoot,

    #[error("ADT resource URI cannot contain a query or fragment")]
    QueryOrFragment,

    #[error(transparent)]
    Url(#[from] url::ParseError),
}

/// A validated, root-relative resource URI in SAP's `/sap/bc` namespace.
///
/// Relative values are resolved beneath [`ADT_ROOT`]. Root-relative values can
/// also address related resources, such as `/sap/bc/esproxy`, advertised by
/// central ADT discovery.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AdtUri(String);

impl AdtUri {
    pub fn parse(value: &str) -> Result<Self, AdtUriError> {
        if value.is_empty() {
            return Err(AdtUriError::Empty);
        }
        if value.trim() != value || value.chars().any(char::is_control) || value.contains('\\') {
            return Err(AdtUriError::InvalidCharacters);
        }
        if value.contains('?') || value.contains('#') {
            return Err(AdtUriError::QueryOrFragment);
        }
        if value.starts_with("//") || Url::parse(value).is_ok() {
            return Err(AdtUriError::Absolute);
        }

        let base = Url::parse(&format!("{VALIDATION_ORIGIN}{ADT_ROOT}/"))?;
        let candidate = if value.starts_with('/') {
            base.join(value)?
        } else if value == &ADT_RESOURCE_ROOT[1..] || value.starts_with("sap/bc/") {
            base.join(&format!("/{value}"))?
        } else {
            base.join(value)?
        };

        if candidate.origin() != base.origin()
            || !(candidate.path() == ADT_RESOURCE_ROOT
                || candidate
                    .path()
                    .starts_with(&format!("{ADT_RESOURCE_ROOT}/")))
        {
            return Err(AdtUriError::OutsideRoot);
        }

        Ok(Self(candidate.path().to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AdtUri {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl TryFrom<&str> for AdtUri {
    type Error = AdtUriError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_adt_resource_paths() {
        assert_eq!(
            AdtUri::parse("programs/programs").unwrap().as_str(),
            "/sap/bc/adt/programs/programs"
        );
        assert_eq!(
            AdtUri::parse("/sap/bc/adt/core/discovery")
                .unwrap()
                .as_str(),
            "/sap/bc/adt/core/discovery"
        );
        assert_eq!(
            AdtUri::parse("/sap/bc/esproxy/semanticcontracts")
                .unwrap()
                .as_str(),
            "/sap/bc/esproxy/semanticcontracts"
        );
    }

    #[test]
    fn rejects_untrusted_targets() {
        for target in [
            "https://attacker.example/sap/bc/adt/core/discovery",
            "//attacker.example/sap/bc/adt/core/discovery",
            "/sap/public/bc/icf/logoff",
            "../../public/bc/icf/logoff",
            "/sap/bc/adt/core/discovery?redirect=1",
        ] {
            assert!(AdtUri::parse(target).is_err(), "accepted {target}");
        }
    }
}
