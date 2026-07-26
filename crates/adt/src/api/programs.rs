use crate::{
    client::{Client, Discovered},
    error::{IncludeError, OperationError, ProgramError, ResponseError},
    models::{Include, Program, parse_include, parse_program},
    operation::{Operation, Stateless},
    protocol::{AdtRequest, AdtResponse},
    resource::{IncludeRef, ObjectVersion, ProgramRef},
    vocabulary::{INCLUDES, PROGRAMS, query_parameter},
};
use derive_builder::Builder;
use http::{HeaderValue, Method, StatusCode, header};

/// A program descriptor tagged with the media-type version returned by SAP.
///
/// TODO: Does it make sense to handle NotModified at the same level as versions?..
#[derive(Clone, Debug)]
pub enum ProgramResponse {
    NotModified { etag: Option<String> },
    V2(Box<Program>),
    V3(Box<Program>),
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
            Self::V2(program) | Self::V3(program) => Some(program.as_ref()),
        }
    }

    /// Consumes the response and returns the descriptor when it was modified.
    pub fn into_program(self) -> Option<Program> {
        match self {
            Self::NotModified { .. } => None,
            Self::V2(program) | Self::V3(program) => Some(*program),
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

/// A conditional response containing an ABAP include descriptor.
#[derive(Clone, Debug)]
pub enum IncludeResponse {
    NotModified { etag: Option<String> },
    V2(Box<Include>),
}

impl IncludeResponse {
    /// Returns the response media-type version.
    pub fn media_version(&self) -> Option<IncludeMediaVersion> {
        match self {
            Self::NotModified { .. } => None,
            Self::V2(_) => Some(IncludeMediaVersion::V2),
        }
    }

    /// Borrows the include descriptor when it was modified.
    pub fn as_include(&self) -> Option<&Include> {
        match self {
            Self::NotModified { .. } => None,
            Self::V2(include) => Some(include.as_ref()),
        }
    }

    /// Consumes the response and returns the descriptor when it was modified.
    pub fn into_include(self) -> Option<Include> {
        match self {
            Self::NotModified { .. } => None,
            Self::V2(include) => Some(*include),
        }
    }

    /// Returns the response entity tag, when present.
    pub fn etag(&self) -> Option<&str> {
        match self {
            Self::NotModified { etag } => etag.as_deref(),
            Self::V2(include) => include.etag.as_deref(),
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
            ProgramMediaVersion::V2 => ProgramResponse::V2(Box::new(program)),
            ProgramMediaVersion::V3 => ProgramResponse::V3(Box::new(program)),
        })
    }
}

/// Fetches the metadata representation of a standalone ABAP include.
#[derive(Builder, Debug)]
#[builder(setter(into))]
#[readonly::make]
pub struct IncludeQuery {
    /// The include resource to fetch.
    pub include: IncludeRef,

    /// Media-type versions in descending caller preference.
    #[builder(default = "default_include_media_priority()")]
    pub priority: Vec<IncludeMediaVersion>,

    /// A cached descriptor ETag used for a conditional query.
    #[builder(setter(strip_option), default)]
    pub etag: Option<String>,

    /// The repository-object version to request.
    #[builder(setter(strip_option), default)]
    pub version: Option<ObjectVersion>,
}

impl Operation<Discovered> for IncludeQuery {
    type Response = IncludeResponse;
    type Kind = Stateless;

    fn request(&self, client: &Client<Discovered>) -> Result<AdtRequest, OperationError> {
        let representation = preferred_include_representation(client, &self.priority)
            .map_err(include_operation_error)?;
        let mut request = AdtRequest::new(Method::GET, self.include.uri().clone());
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
                    .map_err(IncludeError::InvalidEntityTag)
                    .map_err(include_operation_error)?,
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
            return Ok(IncludeResponse::NotModified {
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
            .ok_or(IncludeError::MissingContentType)?;
        let representation =
            IncludeMediaVersion::from_media_type(content_type).ok_or_else(|| {
                IncludeError::UnsupportedContentType {
                    content_type: content_type.to_owned(),
                }
            })?;
        let include = parse_include(
            self.include.clone(),
            response.body(),
            response_etag(&response),
        )?;
        Ok(match representation {
            IncludeMediaVersion::V2 => IncludeResponse::V2(Box::new(include)),
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

impl IncludeRef {
    /// Creates a builder for an operation that fetches this include's metadata.
    pub fn query(&self) -> IncludeQueryBuilder {
        let mut builder = IncludeQueryBuilder::default();
        builder.include(self.clone());
        builder
    }
}

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

/// The SAP media-type version used to decode an include descriptor.
///
/// Only V2 is currently advertised by tested systems, but modeling the version
/// keeps include negotiation aligned with the rest of the programs domain.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IncludeMediaVersion {
    V2,
}

impl IncludeMediaVersion {
    const V2_MEDIA_TYPE: &'static str = "application/vnd.sap.adt.programs.includes.v2+xml";

    fn media_type(self) -> &'static str {
        match self {
            Self::V2 => Self::V2_MEDIA_TYPE,
        }
    }

    fn from_media_type(media_type: &str) -> Option<Self> {
        let essence = media_type.split(';').next()?.trim();
        essence
            .eq_ignore_ascii_case(Self::V2_MEDIA_TYPE)
            .then_some(Self::V2)
    }
}

// TODO: Probably better to handle inside the execution
fn default_program_media_priority() -> Vec<ProgramMediaVersion> {
    vec![ProgramMediaVersion::V3, ProgramMediaVersion::V2]
}

fn default_include_media_priority() -> Vec<IncludeMediaVersion> {
    vec![IncludeMediaVersion::V2]
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

fn preferred_include_representation(
    client: &Client<Discovered>,
    priority: &[IncludeMediaVersion],
) -> Result<IncludeMediaVersion, IncludeError> {
    let collection = client
        .capabilities()
        .collection(INCLUDES.scheme, INCLUDES.term)
        .ok_or(IncludeError::MissingCollection)?;
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
        .ok_or_else(|| IncludeError::UnsupportedRepresentation {
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

fn include_operation_error(error: IncludeError) -> OperationError {
    OperationError::Response(ResponseError::Include(error))
}

#[cfg(test)]
mod tests {
    use http::HeaderMap;

    use super::*;

    const PROGRAM_XML: &str = include_str!("../../tests/fixtures/program-z-test.xml");
    const INCLUDE_XML: &str = include_str!("../../tests/fixtures/include-ztest.xml");

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

    fn include_query() -> IncludeQuery {
        IncludeQueryBuilder::default()
            .include(IncludeRef::for_test(
                crate::AdtUri::parse("/sap/bc/adt/programs/includes/ZTEST").unwrap(),
            ))
            .build()
            .unwrap()
    }

    #[test]
    fn include_query_defaults_to_v2() {
        assert_eq!(include_query().priority, [IncludeMediaVersion::V2]);
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

    #[test]
    fn decodes_a_v2_include_response() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/vnd.sap.adt.programs.includes.v2+xml"),
        );
        headers.insert(header::ETAG, HeaderValue::from_static("include-etag"));
        let response = AdtResponse::new(StatusCode::OK, headers, INCLUDE_XML.as_bytes().to_vec());

        let response = include_query().decode(response).unwrap();
        assert!(matches!(response, IncludeResponse::V2(_)));
        assert_eq!(response.etag(), Some("include-etag"));
    }

    #[test]
    fn returns_not_modified_for_a_current_include_etag() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ETAG, HeaderValue::from_static("include-etag"));
        let response = AdtResponse::new(StatusCode::NOT_MODIFIED, headers, Vec::new());

        let response = include_query().decode(response).unwrap();
        assert!(matches!(response, IncludeResponse::NotModified { .. }));
        assert_eq!(response.etag(), Some("include-etag"));
        assert!(response.as_include().is_none());
    }
}
