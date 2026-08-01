use std::collections::HashMap;

use super::properties::ObjectPropertiesQuery;
use crate::{
    AdtUri, AdtUriError,
    client::{Client, Ready},
    error::{ObjectError, OperationError, ResponseError},
    models::ProgramRunResult,
    objects::{Include, ObjectRef, Program},
    operation::{Operation, Stateless, Unconditional},
    protocol::{AdtRequest, AdtResponse},
    target::TemplateTarget,
    vocabulary::{PROGRAM_RUN, PROGRAM_RUN_RELATION, media_type, query_parameter},
};
use derive_builder::Builder;
use http::Method;
use stduritemplate::Value;
use url::Url;

const PROGRAM_NAME_VARIABLE: &str = "programname";

/// Fetches program properties using the generic object-properties protocol.
pub type ProgramPropertiesQuery<M = Unconditional> = ObjectPropertiesQuery<Program, M>;

/// Runs an executable ABAP program and returns its rendered console output.
///
/// This does not currently support IF_OO_ADT_CLASSRUN inside programs. The
/// only way output is returned is when executing a list report. The backend
/// resource then exports that list into the plain text of the body.
///
/// Even if the user does not have sufficent permissions to execute the
/// program or the program could not be found, 200 OK is returned.
///
/// ADT can not handle program dumps, it simply returns a status code 500.
///
/// The profiler id usually seems to be a URL pointing to a freshly created
/// configuration posted to `runtime/traces/abaptraces/parameters`, there
/// seems to be a way to have them predefined too. Must be clarified
///
/// - Backend handler: `CL_SEDI_ADT_PROGRAMRUN`
#[derive(Builder, Debug)]
#[builder(setter(into))]
#[readonly::make]
pub struct ProgramRun {
    /// The executable program to run.
    pub program: ObjectRef<Program>,

    /// An optional ABAP profiler trace identifier.
    #[builder(setter(strip_option), default)]
    pub profiler_id: Option<String>,
}

impl ProgramRun {
    const TARGET: TemplateTarget = TemplateTarget::new(PROGRAM_RUN, PROGRAM_RUN_RELATION);
}

impl Operation<Ready> for ProgramRun {
    type Response = ProgramRunResult;
    type Kind = Stateless;

    fn request(&self, client: &Client<Ready>) -> Result<AdtRequest, OperationError> {
        let template = Self::TARGET.template(client)?;
        let uri_name = self.program.name().to_ascii_lowercase();
        let (target, query) =
            expand_program_run_target(template, &uri_name, self.profiler_id.as_deref())
                .map_err(object_operation_error)?;
        let mut request = AdtRequest::new(Method::POST, target);
        for (name, value) in query {
            request.push_query(name, value);
        }
        request.set_accept(media_type::SOURCE);
        Ok(request)
    }

    fn decode(
        &self,
        response: AdtResponse,
        _request_target: &AdtUri,
    ) -> Result<Self::Response, ResponseError> {
        if !response.status().is_success() {
            return Err(ResponseError::UnexpectedStatus {
                status: response.status(),
                body: String::from_utf8_lossy(response.body()).into_owned(),
            });
        }
        let content = String::from_utf8(response.into_body())
            .map_err(ObjectError::InvalidResponseEncoding)?;
        Ok(ProgramRunResult::new(self.program.clone(), content))
    }
}

/// Fetches include properties using the generic object-properties protocol.
pub type IncludePropertiesQuery<M = Unconditional> = ObjectPropertiesQuery<Include, M>;

impl ObjectRef<Program> {
    /// Creates a builder for an operation that runs this program.
    pub fn run(&self) -> ProgramRunBuilder {
        let mut builder = ProgramRunBuilder::default();
        builder.program(self.clone());
        builder
    }
}

fn expand_program_run_target(
    template: &str,
    program_name: &str,
    profiler_id: Option<&str>,
) -> Result<(AdtUri, Vec<(String, String)>), ObjectError> {
    if !template_has_variable(template, PROGRAM_NAME_VARIABLE) {
        return Err(ObjectError::InvalidTemplate {
            template: template.to_owned(),
            reason: format!("missing `{PROGRAM_NAME_VARIABLE}` variable"),
        });
    }
    if profiler_id.is_some() && !template_has_variable(template, query_parameter::PROFILER_ID) {
        return Err(ObjectError::UnsupportedTemplateParameter {
            parameter: query_parameter::PROFILER_ID,
        });
    }

    let mut variables = HashMap::from([(
        PROGRAM_NAME_VARIABLE.to_owned(),
        Value::String(program_name.to_owned()),
    )]);
    if let Some(profiler_id) = profiler_id {
        variables.insert(
            query_parameter::PROFILER_ID.to_owned(),
            Value::String(profiler_id.to_owned()),
        );
    }
    let expanded = stduritemplate::expand(template, &variables).map_err(|error| {
        ObjectError::InvalidTemplate {
            template: template.to_owned(),
            reason: error.to_string(),
        }
    })?;
    parse_program_run_target(&expanded).map_err(|source| ObjectError::InvalidExpandedTarget {
        target: expanded,
        source,
    })
}

fn template_has_variable(template: &str, expected: &str) -> bool {
    let mut remaining = template;
    while let Some(start) = remaining.find('{') {
        remaining = &remaining[start + 1..];
        let Some(end) = remaining.find('}') else {
            return false;
        };
        let expression = &remaining[..end];
        let expression = expression
            .chars()
            .next()
            .filter(|operator| "+#./;?&".contains(*operator))
            .map_or(expression, |operator| &expression[operator.len_utf8()..]);
        if expression.split(',').any(|variable| {
            let variable = variable.strip_suffix('*').unwrap_or(variable);
            variable.split_once(':').map_or(variable, |(name, _)| name) == expected
        }) {
            return true;
        }
        remaining = &remaining[end + 1..];
    }
    false
}

fn parse_program_run_target(
    expanded: &str,
) -> Result<(AdtUri, Vec<(String, String)>), AdtUriError> {
    let (path, query) = match Url::parse(expanded) {
        Ok(url) => {
            if !matches!(url.scheme(), "http" | "https")
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(AdtUriError::Absolute);
            }
            if url.fragment().is_some() {
                return Err(AdtUriError::QueryOrFragment);
            }
            (url.path().to_owned(), url.query().map(str::to_owned))
        }
        Err(url::ParseError::RelativeUrlWithoutBase) => {
            if expanded.starts_with("//") {
                return Err(AdtUriError::Absolute);
            }
            if expanded.contains('#') {
                return Err(AdtUriError::QueryOrFragment);
            }
            expanded.split_once('?').map_or_else(
                || (expanded.to_owned(), None),
                |(path, query)| (path.to_owned(), Some(query.to_owned())),
            )
        }
        Err(error) => return Err(AdtUriError::Url(error)),
    };
    let target = AdtUri::parse(&path)?;
    let query = query
        .map(|query| {
            url::form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect()
        })
        .unwrap_or_default();
    Ok((target, query))
}

fn object_operation_error(error: ObjectError) -> OperationError {
    OperationError::Response(ResponseError::Object(error))
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use http::{HeaderMap, HeaderValue, StatusCode, header};

    use super::*;
    use crate::{
        CompatibilityError, Conditional, EntityTag, IncludeProperties, IncludePropertyVersion,
        NegotiableMediaVersion, ProgramProperties, ProgramPropertiesVersion, vocabulary::PROGRAMS,
    };

    const PROGRAM_XML: &str = include_str!("../../tests/fixtures/program-z-test.xml");
    const INCLUDE_XML: &str = include_str!("../../tests/fixtures/include-ztest.xml");
    struct UnusedTransport;

    #[async_trait]
    impl crate::Transport for UnusedTransport {
        async fn send(&self, _request: AdtRequest) -> Result<AdtResponse, crate::TransportError> {
            unreachable!("request construction tests do not send requests")
        }
    }

    fn ready_client(xml: &[u8]) -> Client<Ready> {
        Client::new(UnusedTransport)
            .with_capabilities(crate::models::parse_capabilities(xml).unwrap())
    }

    fn program_properties_query() -> ProgramPropertiesQuery {
        ObjectRef::<Program>::for_test(
            "Z_TEST",
            crate::AdtUri::parse("/sap/bc/adt/programs/programs/Z_TEST").unwrap(),
        )
        .query()
    }

    fn program_properties_response(representation: ProgramPropertiesVersion) -> AdtResponse {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&format!("{}; charset=utf-8", representation.media_type()))
                .unwrap(),
        );
        headers.insert(header::ETAG, HeaderValue::from_static("program-etag"));
        AdtResponse::new(StatusCode::OK, headers, PROGRAM_XML.as_bytes().to_vec())
    }

    fn include_properties_query() -> IncludePropertiesQuery {
        ObjectRef::<Include>::for_test(
            "ZTEST",
            crate::AdtUri::parse("/sap/bc/adt/programs/includes/ZTEST").unwrap(),
        )
        .query()
    }

    fn program_run() -> ProgramRun {
        ProgramRunBuilder::default()
            .program(ObjectRef::<Program>::for_test(
                "Z_TEST",
                crate::AdtUri::parse("/sap/bc/adt/programs/programs/Z_TEST").unwrap(),
            ))
            .build()
            .unwrap()
    }

    fn request_target() -> AdtUri {
        AdtUri::parse("/sap/bc/adt/test").unwrap()
    }

    #[test]
    fn include_properties_query_defaults_to_v2() {
        assert_eq!(
            include_properties_query().priority,
            [IncludePropertyVersion::V2]
        );
    }

    #[test]
    fn expands_namespaced_program_run_variables() {
        let (target, query) = expand_program_run_target(
            "/sap/bc/adt/programs/programrun/{programname}{?profilerId}",
            "/DMO/PROGRAM",
            Some("TRACE ID"),
        )
        .unwrap();

        assert_eq!(
            target.as_str(),
            "/sap/bc/adt/programs/programrun/%2FDMO%2FPROGRAM"
        );
        assert_eq!(query, [("profilerId".to_owned(), "TRACE ID".to_owned())]);
    }

    #[test]
    fn omits_an_unset_program_run_profiler() {
        let (_, query) = expand_program_run_target(
            "/sap/bc/adt/programs/programrun/{programname}{?profilerId}",
            "Z_TEST",
            None,
        )
        .unwrap();

        assert!(query.is_empty());
    }

    #[test]
    fn rejects_profiling_when_the_template_does_not_advertise_it() {
        let error = expand_program_run_target(
            "/sap/bc/adt/programs/programrun/{programname}",
            "Z_TEST",
            Some("TRACE-ID"),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            ObjectError::UnsupportedTemplateParameter { parameter }
                if parameter == query_parameter::PROFILER_ID
        ));
    }

    #[test]
    fn rejects_non_utf8_program_run_output() {
        let response = AdtResponse::new(StatusCode::OK, HeaderMap::new(), vec![0xff]);
        let error = program_run()
            .decode(response, &request_target())
            .unwrap_err();

        assert!(matches!(
            error,
            ResponseError::Object(ObjectError::InvalidResponseEncoding(_))
        ));
    }

    #[test]
    fn program_run_request_requires_the_discovery_collection() {
        let client = ready_client(
            br#"<app:service xmlns:app="http://www.w3.org/2007/app"
                    xmlns:atom="http://www.w3.org/2005/Atom">
                    <app:workspace><atom:title>Programs</atom:title></app:workspace>
                </app:service>"#,
        );
        let error = program_run().request(&client).unwrap_err();

        assert!(matches!(
            error,
            OperationError::Compatibility(CompatibilityError::MissingCollection(category))
                if category == PROGRAM_RUN
        ));
    }

    #[test]
    fn program_run_request_requires_the_relation_template() {
        let client = ready_client(
            br#"<app:service xmlns:app="http://www.w3.org/2007/app"
                    xmlns:atom="http://www.w3.org/2005/Atom">
                    <app:workspace>
                        <atom:title>Programs</atom:title>
                        <app:collection href="/sap/bc/adt/programs/programrun">
                            <atom:category term="programrun"
                                scheme="http://www.sap.com/adt/categories/programs" />
                        </app:collection>
                    </app:workspace>
                </app:service>"#,
        );
        let error = program_run().request(&client).unwrap_err();

        assert!(matches!(
            error,
            OperationError::Response(ResponseError::Object(ObjectError::MissingTemplate {
                relation: PROGRAM_RUN_RELATION
            }))
        ));
    }

    #[test]
    fn tags_a_v2_program_properties_representation() {
        let representation = program_properties_query()
            .decode(
                program_properties_response(ProgramPropertiesVersion::V2),
                &request_target(),
            )
            .unwrap();
        assert!(matches!(representation, ProgramProperties::V2(_)));
    }

    #[test]
    fn tags_a_v3_program_properties_representation() {
        let representation = program_properties_query()
            .decode(
                program_properties_response(ProgramPropertiesVersion::V3),
                &request_target(),
            )
            .unwrap();
        assert!(matches!(representation, ProgramProperties::V3(_)));
    }

    #[test]
    fn properties_query_requires_a_response_content_type() {
        let response = AdtResponse::new(
            StatusCode::OK,
            HeaderMap::new(),
            PROGRAM_XML.as_bytes().to_vec(),
        );

        let error = program_properties_query()
            .decode(response, &request_target())
            .unwrap_err();

        assert!(matches!(
            error,
            ResponseError::MissingContentType { category } if category == PROGRAMS
        ));
    }

    #[test]
    fn properties_query_reports_an_unsupported_response_content_type() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        let response = AdtResponse::new(StatusCode::OK, headers, PROGRAM_XML.as_bytes().to_vec());

        let error = program_properties_query()
            .decode(response, &request_target())
            .unwrap_err();

        assert!(matches!(
            error,
            ResponseError::UnsupportedContentType {
                category,
                content_type,
                supported,
            } if category == PROGRAMS
                && content_type == "application/json"
                && supported == [
                    ProgramPropertiesVersion::V3.media_type(),
                    ProgramPropertiesVersion::V2.media_type(),
                ]
        ));
    }

    #[test]
    fn wraps_a_modified_conditional_program_properties_query() {
        let response = program_properties_query()
            .if_none_match(EntityTag::from_static("old-etag"))
            .decode(
                program_properties_response(ProgramPropertiesVersion::V3),
                &request_target(),
            )
            .unwrap();

        assert!(matches!(
            response,
            Conditional::Modified(ProgramProperties::V3(_))
        ));
    }

    #[test]
    fn rejects_not_modified_for_an_unconditional_program_properties_query() {
        let response = AdtResponse::new(StatusCode::NOT_MODIFIED, HeaderMap::new(), Vec::new());
        let error = program_properties_query()
            .decode(response, &request_target())
            .unwrap_err();

        assert!(matches!(error, ResponseError::UnexpectedNotModified));
    }

    #[test]
    fn decodes_a_v2_include_properties_representation() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/vnd.sap.adt.programs.includes.v2+xml"),
        );
        headers.insert(header::ETAG, HeaderValue::from_static("include-etag"));
        let response = AdtResponse::new(StatusCode::OK, headers, INCLUDE_XML.as_bytes().to_vec());

        let representation = include_properties_query()
            .decode(response, &request_target())
            .unwrap();
        assert!(matches!(representation, IncludeProperties::V2(_)));
        assert_eq!(representation.etag(), Some("include-etag"));
    }

    #[test]
    fn returns_not_modified_for_a_current_include_etag() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ETAG, HeaderValue::from_static("include-etag"));
        let response = AdtResponse::new(StatusCode::NOT_MODIFIED, headers, Vec::new());

        let response = include_properties_query()
            .if_none_match(EntityTag::from_static("include-etag"))
            .decode(response, &request_target())
            .unwrap();
        assert!(matches!(&response, Conditional::NotModified { .. }));
        assert_eq!(response.not_modified_etag(), Some("include-etag"));
        assert!(response.as_modified().is_none());
    }

    #[test]
    fn rejects_not_modified_for_an_unconditional_include_properties_query() {
        let response = AdtResponse::new(StatusCode::NOT_MODIFIED, HeaderMap::new(), Vec::new());
        let error = include_properties_query()
            .decode(response, &request_target())
            .unwrap_err();

        assert!(matches!(error, ResponseError::UnexpectedNotModified));
    }
}
