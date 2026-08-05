use thiserror::Error;

use crate::{CategoryId, Collection};

/// Describes media-type versions that can participate in content negotiation.
///
/// [`negotiate`] selects between the caller's preferred versions and
/// the representations advertised by the server.
pub trait MediaVersionNegotiation: Copy + Eq + Send + Sync + 'static {
    /// Media-type versions supported by this client.
    const SUPPORTED: &'static [Self];

    /// Returns the media type identifying this version.
    fn media_type(self) -> &'static str;

    /// Selects this client's preferred version accepted by a discovered collection.
    fn negotiate(collection: &Collection) -> Result<Self, CompatibilityError> {
        negotiate(Self::SUPPORTED, collection.accepted_media_types())
    }

    /// Finds the supported version identified by a media type.
    fn from_media_type(media_type: &str) -> Option<Self> {
        Self::SUPPORTED
            .iter()
            .copied()
            .find(|version| version.matches_media_type(media_type))
    }

    /// Reports whether a media type identifies this version.
    ///
    /// The default compares the essence case-insensitively and every parameter
    /// except `charset` by name and value, independently of formatting order.
    fn matches_media_type(self, candidate: &str) -> bool {
        media_types_match(self.media_type(), candidate)
    }
}

struct ParsedMediaType<'a> {
    essence: &'a str,
    parameters: Vec<(&'a str, &'a str)>,
}

fn parse_media_type(value: &str) -> Option<ParsedMediaType<'_>> {
    let mut parts = value.split(';');
    let essence = parts.next()?.trim();
    if essence.is_empty() {
        return None;
    }

    let parameters = parts
        .map(|parameter| {
            let (name, value) = parameter.split_once('=')?;
            let name = name.trim();
            let value = value.trim();
            (!name.is_empty() && !value.is_empty()).then_some((name, value))
        })
        .collect::<Option<Vec<_>>>()?;

    Some(ParsedMediaType {
        essence,
        parameters,
    })
}

fn media_types_match(expected: &str, candidate: &str) -> bool {
    let (Some(expected), Some(candidate)) =
        (parse_media_type(expected), parse_media_type(candidate))
    else {
        return false;
    };

    if !expected.essence.eq_ignore_ascii_case(candidate.essence) {
        return false;
    }

    let expected_count = expected
        .parameters
        .iter()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("charset"))
        .count();
    let candidate_count = candidate
        .parameters
        .iter()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("charset"))
        .count();

    expected_count == candidate_count
        && expected
            .parameters
            .iter()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("charset"))
            .all(|(expected_name, expected_value)| {
                candidate
                    .parameters
                    .iter()
                    .any(|(candidate_name, candidate_value)| {
                        expected_name.eq_ignore_ascii_case(candidate_name)
                            && expected_value == candidate_value
                    })
            })
}

/// Finds the first preferred media type accepted by the backend.
pub fn negotiate<V>(preferred: &[V], accepted: &[String]) -> Result<V, CompatibilityError>
where
    V: MediaVersionNegotiation,
{
    let candidates: Vec<V> = accepted
        .iter()
        .map(String::as_str)
        .filter_map(V::from_media_type)
        .collect();

    preferred
        .iter()
        .copied()
        .find(|version| candidates.contains(version))
        .ok_or_else(|| CompatibilityError::NoCompatibleMediaType {
            preferred: preferred
                .iter()
                .map(|version| version.media_type().to_owned())
                .collect(),
            accepted: accepted.to_vec(),
        })
}

/// An error establishing protocol compatibility with an ADT backend.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CompatibilityError {
    /// Central discovery did not advertise a required collection.
    #[error("ADT discovery did not advertise collection {0:?}")]
    MissingCollection(CategoryId),

    /// The client and backend have no mutually supported media type.
    #[error(
        "none of the preferred media types {preferred:?} are accepted by the backend: {accepted:?}"
    )]
    NoCompatibleMediaType {
        preferred: Vec<String>,
        accepted: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Version(&'static str);

    impl Version {
        const V2: Self = Self("application/vnd.test.v2+xml");
        const V3: Self = Self("application/vnd.test.v3+xml");
    }

    impl MediaVersionNegotiation for Version {
        const SUPPORTED: &'static [Self] = &[Self::V3, Self::V2];

        fn media_type(self) -> &'static str {
            self.0
        }
    }

    #[test]
    fn selects_the_first_preferred_media_type_accepted_by_the_backend() {
        let accepted = vec!["application/vnd.test.v2+xml; charset=utf-8".to_owned()];

        let version = negotiate(&[Version::V3, Version::V2], &accepted).unwrap();

        assert_eq!(version, Version::V2);
    }

    #[test]
    fn matches_semantic_parameters_independently_of_charset_and_formatting() {
        let version = Version(
            "application/vnd.sap.as+xml; charset=utf-8; \
             dataname=com.sap.adt.CreateCorrectionRequest.v1",
        );

        assert!(version.matches_media_type(
            "APPLICATION/VND.SAP.AS+XML;dataname=com.sap.adt.CreateCorrectionRequest.v1; \
             charset=UTF-8"
        ));
    }

    #[test]
    fn rejects_different_or_unexpected_semantic_parameters() {
        let legacy = Version(
            "application/vnd.sap.as+xml; \
             dataname=com.sap.adt.CreateCorrectionRequest",
        );
        let versioned =
            "application/vnd.sap.as+xml; dataname=com.sap.adt.CreateCorrectionRequest.v1";

        assert!(!legacy.matches_media_type(versioned));
        assert!(!Version("application/vnd.sap.as+xml").matches_media_type(versioned));
    }

    #[test]
    fn matches_version_media_type_parameters_exactly() {
        let quick_fix = Version("application/vnd.sap.adt.quickfixes.evaluation+xml;version=1.0.0");

        assert!(quick_fix.matches_media_type(
            "application/vnd.sap.adt.quickfixes.evaluation+xml; version=1.0.0"
        ));
        assert!(!quick_fix.matches_media_type(
            "application/vnd.sap.adt.quickfixes.evaluation+xml; version=2.0.0"
        ));
    }

    #[test]
    fn reports_both_sides_when_no_media_type_is_compatible() {
        let accepted = vec!["application/vnd.test.v1+xml".to_owned()];

        let error = negotiate(&[Version::V3, Version::V2], &accepted).unwrap_err();

        assert!(matches!(
            error,
            CompatibilityError::NoCompatibleMediaType {
                preferred,
                accepted,
            } if preferred == [Version::V3.0, Version::V2.0]
                && accepted == ["application/vnd.test.v1+xml"]
        ));
    }
}
