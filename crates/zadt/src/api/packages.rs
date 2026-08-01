use std::collections::HashMap;

use http::{Method, StatusCode, header};
use stduritemplate::Value;

use super::properties::ObjectPropertiesQuery;
use crate::{
    AdtUri, CategoryId, Client, ObjectCollection, ObjectError, ObjectRef, Operation,
    OperationError, Package, PackageSettings, PackageTree, PackageTreeKind, Ready, ResponseError,
    Stateless, Unconditional,
    protocol::{AdtRequest, AdtResponse},
    target::{CollectionTarget, TemplateTarget},
};

const PACKAGE_TREE_RELATION: &str = "tree";
const PACKAGE_TREE_MEDIA_TYPE: &str = "application/vnd.sap.adt.packages.tree.v1+xml";
const PACKAGE_SETTINGS_MEDIA_TYPE: &str = "application/vnd.sap.adt.packages.settings+xml";
const PACKAGE_SETTINGS: CategoryId = CategoryId {
    scheme: "http://www.sap.com/wbobj/packages",
    term: "settings",
};

/// Fetches package properties using the generic object-properties protocol.
pub type PackagePropertiesQuery<M = Unconditional> = ObjectPropertiesQuery<Package, M>;

/// Fetches either the ancestors or immediate children of a package.
#[derive(Clone, Debug)]
pub struct PackageTreeQuery {
    /// The package at which tree traversal starts.
    pub package: ObjectRef<Package>,

    /// Whether ancestors or immediate subpackages are requested.
    pub kind: PackageTreeKind,
}

impl PackageTreeQuery {
    const TARGET: TemplateTarget = TemplateTarget::new(Package::CATEGORY, PACKAGE_TREE_RELATION);

    /// Creates a package-tree query with the selected direction.
    pub fn new(package: ObjectRef<Package>, kind: PackageTreeKind) -> Self {
        Self { package, kind }
    }
}

impl Operation<Ready> for PackageTreeQuery {
    type Response = PackageTree;
    type Kind = Stateless;

    fn request(&self, client: &Client<Ready>) -> Result<AdtRequest, OperationError> {
        let template = Self::TARGET.template(client)?;
        let (target, query) = expand_tree_target(template, self.package.name(), self.kind)?;
        let mut request = AdtRequest::new(Method::GET, target);
        for (name, value) in query {
            request.push_query(name, value);
        }
        request.set_accept(PACKAGE_TREE_MEDIA_TYPE);
        Ok(request)
    }

    fn decode(
        &self,
        response: AdtResponse,
        request_target: &AdtUri,
    ) -> Result<Self::Response, ResponseError> {
        ensure_xml_response(&response, Package::CATEGORY, PACKAGE_TREE_MEDIA_TYPE)?;
        PackageTree::parse(response.body(), request_target)
    }
}

/// Fetches global package editor settings.
#[derive(Clone, Copy, Debug, Default)]
pub struct PackageSettingsQuery;

impl Operation<Ready> for PackageSettingsQuery {
    type Response = PackageSettings;
    type Kind = Stateless;

    fn request(&self, client: &Client<Ready>) -> Result<AdtRequest, OperationError> {
        let mut request = CollectionTarget::new(PACKAGE_SETTINGS).request(client, Method::GET)?;
        request.set_accept(PACKAGE_SETTINGS_MEDIA_TYPE);
        Ok(request)
    }

    fn decode(
        &self,
        response: AdtResponse,
        _request_target: &AdtUri,
    ) -> Result<Self::Response, ResponseError> {
        ensure_xml_response(&response, PACKAGE_SETTINGS, PACKAGE_SETTINGS_MEDIA_TYPE)?;
        PackageSettings::parse(response.body())
    }
}

impl ObjectRef<Package> {
    /// Creates a query for this package and all of its ancestors.
    pub fn super_tree(&self) -> PackageTreeQuery {
        PackageTreeQuery::new(self.clone(), PackageTreeKind::Super)
    }

    /// Creates a query for this package's immediate subpackages.
    pub fn sub_tree(&self) -> PackageTreeQuery {
        PackageTreeQuery::new(self.clone(), PackageTreeKind::Sub)
    }
}

fn expand_tree_target(
    template: &str,
    package_name: &str,
    kind: PackageTreeKind,
) -> Result<(AdtUri, Vec<(String, String)>), OperationError> {
    let variables = HashMap::from([
        (
            "packagename".to_owned(),
            Value::String(package_name.to_owned()),
        ),
        ("type".to_owned(), Value::String(kind.as_str().to_owned())),
    ]);
    let expanded = stduritemplate::expand(template, &variables).map_err(|error| {
        ResponseError::Object(ObjectError::InvalidTemplate {
            template: template.to_owned(),
            reason: error.to_string(),
        })
    })?;
    let (path, query) = expanded
        .split_once('?')
        .map_or((expanded.as_str(), None), |(path, query)| {
            (path, Some(query))
        });
    let target = AdtUri::parse(path).map_err(|source| {
        ResponseError::Object(ObjectError::InvalidExpandedTarget {
            target: expanded.clone(),
            source,
        })
    })?;
    let query: Vec<(String, String)> = query
        .map(|query| {
            url::form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect()
        })
        .unwrap_or_default();
    for expected in ["packagename", "type"] {
        if !query.iter().any(|(name, _)| name == expected) {
            return Err(ResponseError::Object(ObjectError::InvalidTemplate {
                template: template.to_owned(),
                reason: format!("missing `{expected}` query variable"),
            })
            .into());
        }
    }
    Ok((target, query))
}

fn ensure_xml_response(
    response: &AdtResponse,
    category: CategoryId,
    expected_content_type: &'static str,
) -> Result<(), ResponseError> {
    if response.status() != StatusCode::OK {
        return Err(ResponseError::UnexpectedStatus {
            status: response.status(),
            body: String::from_utf8_lossy(response.body()).into_owned(),
        });
    }
    let Some(content_type) = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    else {
        return Err(ResponseError::MissingContentType { category });
    };
    let matches = content_type
        .split(';')
        .next()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case(expected_content_type));
    if !matches {
        return Err(ResponseError::UnsupportedContentType {
            category,
            content_type: content_type.to_owned(),
            supported: vec![expected_content_type.to_owned()],
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use http::{HeaderMap, HeaderValue};

    use super::*;

    #[test]
    fn expands_namespaced_package_tree_targets() {
        let (target, query) = expand_tree_target(
            "/sap/bc/adt/packages/$tree{?packagename,type}",
            "/DMO/FLIGHT",
            PackageTreeKind::Super,
        )
        .unwrap();

        assert_eq!(target.as_str(), "/sap/bc/adt/packages/$tree");
        assert_eq!(
            query,
            [
                ("packagename".to_owned(), "/DMO/FLIGHT".to_owned()),
                ("type".to_owned(), "super".to_owned()),
            ]
        );
    }

    #[test]
    fn rejects_a_tree_template_without_required_variables() {
        let error = expand_tree_target(
            "/sap/bc/adt/packages/$tree{?packagename}",
            "SADT_MAIN",
            PackageTreeKind::Sub,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            OperationError::Response(ResponseError::Object(ObjectError::InvalidTemplate {
                reason,
                ..
            })) if reason.contains("`type`")
        ));
    }

    #[test]
    fn rejects_an_unexpected_settings_content_type() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/xml"),
        );
        let response = AdtResponse::new(StatusCode::OK, headers, Vec::new());

        let error = ensure_xml_response(&response, PACKAGE_SETTINGS, PACKAGE_SETTINGS_MEDIA_TYPE)
            .unwrap_err();

        assert!(matches!(
            error,
            ResponseError::UnsupportedContentType { content_type, .. }
                if content_type == "application/xml"
        ));
    }
}
