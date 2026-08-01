use http::Method;

use crate::{
    AdtRequest, AdtUri, CategoryId, Client, Collection, ObjectError, OperationError, Ready,
    ResponseError,
};

pub(crate) const CENTRAL_DISCOVERY: FixedTarget = FixedTarget::new("/sap/bc/adt/discovery");
pub(crate) const CORE_DISCOVERY: FixedTarget = FixedTarget::new("/sap/bc/adt/core/discovery");
pub(crate) const HTTP_SESSIONS: FixedTarget = FixedTarget::new("/sap/bc/adt/core/http/sessions");

/// A protocol bootstrap target known without server discovery.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FixedTarget(&'static str);

impl FixedTarget {
    pub(crate) const fn new(target: &'static str) -> Self {
        Self(target)
    }

    #[cfg(feature = "reqwest")]
    pub(crate) const fn as_str(self) -> &'static str {
        self.0
    }

    pub(crate) fn uri(self) -> AdtUri {
        AdtUri::parse(self.0).expect("a fixed ADT target must be a valid ADT URI")
    }

    pub(crate) fn request(self, method: Method) -> AdtRequest {
        AdtRequest::new(method, self.uri())
    }
}

/// A collection identified by its stable category in central discovery.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CollectionTarget(CategoryId);

impl CollectionTarget {
    pub(crate) const fn new(category: CategoryId) -> Self {
        Self(category)
    }

    pub(crate) fn collection(self, client: &Client<Ready>) -> Result<&Collection, OperationError> {
        client.require_collection(self.0).map_err(Into::into)
    }

    pub(crate) fn request(
        self,
        client: &Client<Ready>,
        method: Method,
    ) -> Result<AdtRequest, OperationError> {
        let target = self.collection(client)?.target().clone();
        Ok(AdtRequest::new(method, target))
    }
}

/// A relation template advertised by a discovered collection.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TemplateTarget {
    collection: CollectionTarget,
    relation: &'static str,
}

impl TemplateTarget {
    pub(crate) const fn new(category: CategoryId, relation: &'static str) -> Self {
        Self {
            collection: CollectionTarget::new(category),
            relation,
        }
    }

    pub(crate) fn template(self, client: &Client<Ready>) -> Result<&str, OperationError> {
        self.collection
            .collection(client)?
            .template_links()
            .iter()
            .find(|link| link.relation() == self.relation)
            .map(|link| link.template())
            .ok_or_else(|| {
                OperationError::Response(ResponseError::Object(ObjectError::MissingTemplate {
                    relation: self.relation,
                }))
            })
    }
}
