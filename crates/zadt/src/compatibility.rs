//! Negotiation and validation of compatibility between the client and ADT backends.
//!
//! This includes media-type negotiation and advertised compatibility graphs,
//! which can differ between SAP releases.

use thiserror::Error;

use crate::{CategoryId, Collection};

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

/// Describes media-type versions that can participate in content negotiation.
///
/// [`negotiate`] selects between the caller's preferred versions and
/// the representations advertised by the server.
pub trait NegotiableMediaVersion: Copy + Eq + Send + Sync + 'static {
    /// Media-type versions supported by this client.
    const SUPPORTED: &'static [Self];

    /// Returns the media-type essence identifying this version.
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
    fn matches_media_type(self, candidate: &str) -> bool {
        candidate
            .split(';')
            .next()
            .is_some_and(|v| v.trim().eq_ignore_ascii_case(self.media_type()))
    }
}

/// Finds the first preferred media type accepted by the backend.
pub fn negotiate<V>(preferred: &[V], accepted: &[String]) -> Result<V, CompatibilityError>
where
    V: NegotiableMediaVersion,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Version(&'static str);

    impl Version {
        const V2: Self = Self("application/vnd.test.v2+xml");
        const V3: Self = Self("application/vnd.test.v3+xml");
    }

    impl NegotiableMediaVersion for Version {
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
