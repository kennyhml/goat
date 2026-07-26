use derive_builder::Builder;
use http::{HeaderValue, Method, StatusCode, header};

use crate::{
    client::{Client, Discovered},
    error::{OperationError, ProgramError, ResponseError},
    models::{Program, parse_program},
    operation::{Operation, Stateless},
    protocol::{AdtRequest, AdtResponse},
    resource::{ObjectVersion, ProgramRef},
    vocabulary::{PROGRAMS, query_parameter},
};

/// The SAP media-type version used to decode a program descriptor.
///
/// `ProgramQuery` defaults to V3 before V2, and callers can supply a different
/// preference. Both representations are normalized into [`Program`], because
/// there seems to be no difference between V2 and V3.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ProgramMediaVersion {
    V2,
    V3,
}

impl ProgramMediaVersion {
    const V2_MEDIA_TYPE: &'static str = "application/vnd.sap.adt.programs.programs.v2+xml";
    const V3_MEDIA_TYPE: &'static str = "application/vnd.sap.adt.programs.programs.v3+xml";

    fn media_type(self) -> &'static str {
        match self {
            Self::V2 => Self::V2_MEDIA_TYPE,
            Self::V3 => Self::V3_MEDIA_TYPE,
        }
    }

    fn from_media_type(media_type: &str) -> Option<Self> {
        let essence = media_type.split(';').next()?.trim();
        if essence.eq_ignore_ascii_case(Self::V3_MEDIA_TYPE) {
            Some(Self::V3)
        } else if essence.eq_ignore_ascii_case(Self::V2_MEDIA_TYPE) {
            Some(Self::V2)
        } else {
            None
        }
    }
}

/// A program descriptor tagged with the media-type version returned by SAP.
#[derive(Clone, Debug)]
pub enum ProgramResponse {
    /// The supplied ETag still identifies the current program descriptor.
    NotModified {
        /// The entity tag returned by SAP, when present.
        etag: Option<String>,
    },

    /// A V2 program representation.
    V2(Program),

    /// A V3 program representation.
    V3(Program),
}

impl ProgramResponse {
    /// Returns the response media-type version.
    pub fn media_version(&self) -> Option<ProgramMediaVersion> {
        match self {
            Self::NotModified { .. } => None,
            Self::V2(_) => Some(ProgramMediaVersion::V2),
            Self::V3(_) => Some(ProgramMediaVersion::V3),
        }
    }

    /// Borrows the normalized program descriptor when it was modified.
    pub fn as_program(&self) -> Option<&Program> {
        match self {
            Self::NotModified { .. } => None,
            Self::V2(program) | Self::V3(program) => Some(program),
        }
    }

    /// Consumes the response and returns the descriptor when it was modified.
    pub fn into_program(self) -> Option<Program> {
        match self {
            Self::NotModified { .. } => None,
            Self::V2(program) | Self::V3(program) => Some(program),
        }
    }

    /// Returns the response entity tag, when present.
    pub fn etag(&self) -> Option<&str> {
        match self {
            Self::NotModified { etag } => etag.as_deref(),
            Self::V2(program) | Self::V3(program) => program.etag.as_deref(),
        }
    }
}

/// Fetches and normalizes the metadata representation of a program.
#[derive(Builder, Debug)]
#[builder(setter(into))]
#[readonly::make]
pub struct ProgramQuery {
    /// The program resource to fetch.
    pub program: ProgramRef,

    /// Media-type versions in descending caller preference.
    #[builder(default = "default_program_media_priority()")]
    pub priority: Vec<ProgramMediaVersion>,

    /// A cached descriptor ETag used for a conditional query.
    #[builder(setter(strip_option), default)]
    pub etag: Option<String>,

    /// The repository-object version to request.
    #[builder(setter(strip_option), default)]
    pub version: Option<ObjectVersion>,
}

impl Operation<Discovered> for ProgramQuery {
    type Response = ProgramResponse;
    type Kind = Stateless;

    fn request(&self, client: &Client<Discovered>) -> Result<AdtRequest, OperationError> {
        let representation =
            preferred_representation(client, &self.priority).map_err(program_operation_error)?;
        let mut request = AdtRequest::new(Method::GET, self.program.uri().clone());
        if let Some(version) = self.version {
            request.push_query(query_parameter::VERSION, version.as_str());
        }
        request.headers_mut().insert(
            header::ACCEPT,
            HeaderValue::from_static(representation.media_type()),
        );
        if let Some(etag) = &self.etag {
            request.headers_mut().insert(
                header::IF_NONE_MATCH,
                HeaderValue::from_str(etag)
                    .map_err(ProgramError::InvalidEntityTag)
                    .map_err(program_operation_error)?,
            );
        } else {
            request
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        }
        Ok(request)
    }

    fn decode(&self, response: AdtResponse) -> Result<Self::Response, ResponseError> {
        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok(ProgramResponse::NotModified {
                etag: response_etag(&response),
            });
        }
        if response.status() != StatusCode::OK {
            return Err(ResponseError::UnexpectedStatus {
                status: response.status(),
                body: String::from_utf8_lossy(response.body()).into_owned(),
            });
        }

        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .ok_or(ProgramError::MissingContentType)?;
        let representation =
            ProgramMediaVersion::from_media_type(content_type).ok_or_else(|| {
                ProgramError::UnsupportedContentType {
                    content_type: content_type.to_owned(),
                }
            })?;
        let program = parse_program(
            self.program.clone(),
            response.body(),
            response_etag(&response),
        )?;
        Ok(match representation {
            ProgramMediaVersion::V2 => ProgramResponse::V2(program),
            ProgramMediaVersion::V3 => ProgramResponse::V3(program),
        })
    }
}

impl ProgramRef {
    /// Creates a builder for an operation that fetches this programs metadata.
    ///
    /// That way, it becomes possible to just call
    /// ```rust,ignore
    /// program.query().etag(etag).execute(&client).await?
    /// ```
    /// instead of constructing an operation from scratch:
    /// ```rust,ignore
    /// ProgramQueryBuilder::default()
    ///     .program(program)
    ///     .etag(etag)
    ///     .execute(&client)
    ///     .await?
    /// ```
    pub fn query(&self) -> ProgramQueryBuilder {
        let mut builder = ProgramQueryBuilder::default();
        builder.program(self.clone());
        builder
    }
}

// TODO: Probably better to handle inside the execution
fn default_program_media_priority() -> Vec<ProgramMediaVersion> {
    vec![ProgramMediaVersion::V3, ProgramMediaVersion::V2]
}

// TODO: Move this to common utils or even into AdtResponse
fn response_etag(response: &AdtResponse) -> Option<String> {
    response
        .headers()
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

// TODO: Negotiate woud be the more accurate term and this is a basic
// operation we should also have some generic helper for, parse both
// sides into one representation and then have them implement PartialOrd
// or something
fn preferred_representation(
    client: &Client<Discovered>,
    priority: &[ProgramMediaVersion],
) -> Result<ProgramMediaVersion, ProgramError> {
    let collection = client
        .capabilities()
        .collection(PROGRAMS.scheme, PROGRAMS.term)
        .ok_or(ProgramError::MissingCollection)?;
    let accepted = collection.accepted_media_types();

    priority
        .iter()
        .copied()
        .find(|representation| {
            accepted.iter().any(|media_type| {
                media_type.split(';').next().is_some_and(|value| {
                    value
                        .trim()
                        .eq_ignore_ascii_case(representation.media_type())
                })
            })
        })
        .ok_or_else(|| ProgramError::UnsupportedRepresentation {
            preferred: priority
                .iter()
                .map(|representation| representation.media_type().to_owned())
                .collect(),
            accepted: accepted.to_vec(),
        })
}

fn program_operation_error(error: ProgramError) -> OperationError {
    OperationError::Response(ResponseError::Program(error))
}

#[cfg(test)]
mod tests {
    use http::HeaderMap;

    use super::*;

    const PROGRAM_XML: &str = include_str!("../../tests/fixtures/program-z-test.xml");

    fn program_query() -> ProgramQuery {
        ProgramQueryBuilder::default()
            .program(ProgramRef::for_test(
                crate::AdtUri::parse("/sap/bc/adt/programs/programs/Z_TEST").unwrap(),
            ))
            .build()
            .unwrap()
    }

    fn program_response(representation: ProgramMediaVersion) -> AdtResponse {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&format!("{}; charset=utf-8", representation.media_type()))
                .unwrap(),
        );
        headers.insert(header::ETAG, HeaderValue::from_static("program-etag"));
        AdtResponse::new(StatusCode::OK, headers, PROGRAM_XML.as_bytes().to_vec())
    }

    #[test]
    fn tags_a_v2_program_response() {
        let response = program_query()
            .decode(program_response(ProgramMediaVersion::V2))
            .unwrap();
        assert!(matches!(response, ProgramResponse::V2(_)));
    }

    #[test]
    fn tags_a_v3_program_response() {
        let response = program_query()
            .decode(program_response(ProgramMediaVersion::V3))
            .unwrap();
        assert!(matches!(response, ProgramResponse::V3(_)));
    }
}
