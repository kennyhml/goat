use derive_builder::Builder;
use http::{Method, StatusCode};

use crate::{
    client::{Client, ClientState},
    error::{ObjectError, OperationError, ResponseError},
    models::{AccessMode, LockHandle, SourceCode, parse_lock_handle},
    objects::{ObjectRef, ObjectType, Source},
    operation::{Operation, Stateful, Stateless},
    protocol::{AdtRequest, AdtResponse},
    resource::SourceRef,
    vocabulary::{PostAction, media_type, query_parameter},
};

/// Fetches the source code advertised by a [`SourceRef`].
#[derive(Debug)]
pub struct ObjectSourceQuery {
    /// The source resource to fetch.
    pub source: SourceRef,
}

impl<S: ClientState> Operation<S> for ObjectSourceQuery {
    type Response = SourceCode;
    type Kind = Stateless;

    fn request(&self, _client: &Client<S>) -> Result<AdtRequest, OperationError> {
        let mut request = AdtRequest::new(Method::GET, self.source.uri.clone());
        for (name, value) in &self.source.query {
            request.push_query(name, value);
        }
        request.set_accept(media_type::SOURCE);
        Ok(request)
    }

    fn decode(
        &self,
        response: AdtResponse,
        _request_target: &crate::AdtUri,
    ) -> Result<Self::Response, ResponseError> {
        expect_ok(&response)?;
        let etag = response.entity_tag();
        let content = String::from_utf8(response.into_body())
            .map_err(ObjectError::InvalidResponseEncoding)?;
        Ok(SourceCode::new(self.source.clone(), content, etag))
    }
}

impl AccessMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Show => "SHOW",
            Self::Modify => "MODIFY",
        }
    }
}

// Every statically identified object supports locking.
impl<T: ObjectType> ObjectRef<T> {
    /// Creates an object-lock operation.
    pub fn lock(&self, access_mode: AccessMode) -> LockRequest {
        LockRequest::new(self.erase(), access_mode)
    }

    /// Creates an operation that releases this object's lock.
    pub fn unlock(&self, lock_handle: LockHandle) -> Result<UnlockRequest, ObjectError> {
        if self.uri() != lock_handle.object().uri() {
            return Err(ObjectError::LockHandleObjectMismatch {
                expected: self.to_string(),
                actual: lock_handle.object().to_string(),
            });
        }
        Ok(UnlockRequest::new(lock_handle))
    }
}

impl<T: Source> ObjectRef<T> {
    /// Returns the objects conventional source resource.
    pub fn source(&self) -> SourceRef {
        let uri = self
            .uri()
            .append_segments(T::PATH)
            .expect("static source path segments form a valid ADT URI");
        SourceRef::new(self.erase(), uri)
    }
}

/// Locks a repository object within a [`crate::UserSession`].
///
/// The operation sends `POST` with `_action=LOCK` and the configured
/// `accessMode`. The returned [`LockHandle`] must remain in the same user
/// session as subsequent update or unlock operations.
#[derive(Debug)]
pub struct LockRequest {
    /// The repository object to lock.
    pub object: ObjectRef,

    /// Whether the object is locked for display or modification.
    pub access_mode: AccessMode,
}

impl LockRequest {
    pub(crate) fn new(object: ObjectRef, access_mode: AccessMode) -> Self {
        Self {
            object,
            access_mode,
        }
    }
}

impl<S: ClientState> Operation<S> for LockRequest {
    type Response = LockHandle;
    type Kind = Stateful;

    fn request(&self, _client: &Client<S>) -> Result<AdtRequest, OperationError> {
        let mut request = AdtRequest::new(Method::POST, self.object.uri().clone());
        request.push_query(query_parameter::ACTION, PostAction::Lock.as_str());
        request.push_query(query_parameter::ACCESS_MODE, self.access_mode.as_str());
        request.set_accept(media_type::LOCK_RESULT);
        Ok(request)
    }

    fn decode(
        &self,
        response: AdtResponse,
        _request_target: &crate::AdtUri,
    ) -> Result<Self::Response, ResponseError> {
        expect_ok(&response)?;
        let handle = parse_lock_handle(response.body())?;
        Ok(LockHandle::new(self.object.clone(), handle))
    }
}

/// Releases a [`LockHandle`] within its SAP user session.
#[derive(Debug)]
pub struct UnlockRequest {
    /// The lock to release.
    pub lock_handle: LockHandle,
}

impl UnlockRequest {
    pub(crate) fn new(lock_handle: LockHandle) -> Self {
        Self { lock_handle }
    }
}

impl<S: ClientState> Operation<S> for UnlockRequest {
    type Response = ();
    type Kind = Stateful;

    fn request(&self, _client: &Client<S>) -> Result<AdtRequest, OperationError> {
        let mut request = AdtRequest::new(Method::POST, self.lock_handle.object().uri().clone());
        request.push_query(query_parameter::ACTION, PostAction::Unlock.as_str());
        request.push_query(query_parameter::LOCK_HANDLE, self.lock_handle.handle());
        Ok(request)
    }

    fn decode(
        &self,
        response: AdtResponse,
        _request_target: &crate::AdtUri,
    ) -> Result<Self::Response, ResponseError> {
        expect_ok(&response)
    }
}

/// Replaces the complete source code of an object.
///
/// This operation is stateful and requires a [`LockHandle`] issued for the
/// object being updated. The builder verifies this relationship.
#[derive(Builder, Debug)]
#[builder(setter(into), build_fn(validate = Self::validate))]
pub struct ObjectSourceUpdate {
    /// The source resource whose complete content will be replaced.
    source: SourceRef,

    /// A modification lock obtained for the source's owning object.
    lock_handle: LockHandle,

    /// The complete replacement source text.
    content: String,
}

impl ObjectSourceUpdate {
    /// Returns the source resource that will be replaced.
    pub fn source(&self) -> &SourceRef {
        &self.source
    }

    /// Returns the lock authorizing this update.
    pub fn lock_handle(&self) -> &LockHandle {
        &self.lock_handle
    }

    /// Returns the complete replacement source text.
    pub fn content(&self) -> &str {
        &self.content
    }
}

impl ObjectSourceUpdateBuilder {
    fn validate(&self) -> Result<(), String> {
        if let (Some(source), Some(lock_handle)) = (&self.source, &self.lock_handle)
            && &source.object != lock_handle.object()
        {
            return Err(format!(
                "lock for `{}` cannot update source `{}`",
                lock_handle.object(),
                source
            ));
        }
        Ok(())
    }
}

impl<S: ClientState> Operation<S> for ObjectSourceUpdate {
    type Response = ();
    type Kind = Stateful;

    fn request(&self, _client: &Client<S>) -> Result<AdtRequest, OperationError> {
        let mut request = AdtRequest::new(Method::PUT, self.source.uri.clone());
        for (name, value) in &self.source.query {
            request.push_query(name, value);
        }
        request.push_query(query_parameter::LOCK_HANDLE, self.lock_handle.handle());
        request.set_content_type(media_type::SOURCE_UPDATE);
        request.set_body(self.content.clone());
        Ok(request)
    }

    fn decode(
        &self,
        response: AdtResponse,
        _request_target: &crate::AdtUri,
    ) -> Result<Self::Response, ResponseError> {
        expect_ok(&response)
    }
}

impl LockHandle {
    /// Consumes this handle and creates an operation that removes the lock.
    pub fn remove(self) -> UnlockRequest {
        UnlockRequest::new(self)
    }
}

impl SourceRef {
    /// Creates a stateless query for this source representation.
    pub fn query(&self) -> ObjectSourceQuery {
        ObjectSourceQuery {
            source: self.clone(),
        }
    }

    /// Creates an update builder pre-populated with this source resource.
    pub fn update(&self) -> ObjectSourceUpdateBuilder {
        let mut builder = ObjectSourceUpdateBuilder::default();
        builder.source(self.clone());
        builder
    }
}

fn expect_ok(response: &AdtResponse) -> Result<(), ResponseError> {
    if response.status() == StatusCode::OK {
        Ok(())
    } else {
        Err(ResponseError::UnexpectedStatus {
            status: response.status(),
            body: String::from_utf8_lossy(response.body()).into_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Class, ClassSourceComponent, ObjectRef, Program};

    #[test]
    fn object_operations_do_not_require_discovery() {
        fn accepts_initial<O: Operation<crate::Initial>>() {}

        accepts_initial::<ObjectSourceQuery>();
        accepts_initial::<LockRequest>();
        accepts_initial::<UnlockRequest>();
        accepts_initial::<ObjectSourceUpdate>();
    }

    #[test]
    fn derives_the_conventional_source_from_a_program_reference() {
        let program = ObjectRef::<Program>::for_test(
            "ZPROGRAM",
            crate::AdtUri::parse("/sap/bc/adt/programs/programs/ZPROGRAM").unwrap(),
        );

        assert_eq!(
            program.source().uri.as_str(),
            "/sap/bc/adt/programs/programs/ZPROGRAM/source/main"
        );
    }

    #[test]
    fn derives_class_source_component_resources() {
        let class = ObjectRef::<Class>::for_test(
            "ZCL_EXAMPLE",
            crate::AdtUri::parse("/sap/bc/adt/oo/classes/zcl_example").unwrap(),
        );

        for (component, suffix) in [
            (ClassSourceComponent::Main, "source/main"),
            (ClassSourceComponent::Definitions, "includes/definitions"),
            (
                ClassSourceComponent::Implementations,
                "includes/implementations",
            ),
            (ClassSourceComponent::Macros, "includes/macros"),
            (ClassSourceComponent::TestClasses, "includes/testclasses"),
        ] {
            let source = class.component_source(component);
            assert_eq!(
                source.uri.as_str(),
                format!("/sap/bc/adt/oo/classes/zcl_example/{suffix}")
            );
            assert_eq!(source.object, class.erase());
        }
    }

    #[test]
    fn update_builder_rejects_a_lock_for_another_object() {
        let first = ObjectRef::<Program>::for_test(
            "ZFIRST",
            crate::AdtUri::parse("/sap/bc/adt/programs/programs/ZFIRST").unwrap(),
        );
        let second = ObjectRef::<Program>::for_test(
            "ZSECOND",
            crate::AdtUri::parse("/sap/bc/adt/programs/programs/ZSECOND").unwrap(),
        );
        let lock_handle = LockHandle::new(first.erase(), "LOCK-HANDLE".to_owned());

        let error = second
            .source()
            .update()
            .lock_handle(lock_handle)
            .content("REPORT zsecond.")
            .build()
            .unwrap_err();

        assert!(error.to_string().contains("cannot update source"));
    }

    #[test]
    fn object_rejects_another_objects_lock_for_unlock() {
        let first = ObjectRef::<Program>::for_test(
            "ZFIRST",
            crate::AdtUri::parse("/sap/bc/adt/programs/programs/ZFIRST").unwrap(),
        );
        let second = ObjectRef::<Program>::for_test(
            "ZSECOND",
            crate::AdtUri::parse("/sap/bc/adt/programs/programs/ZSECOND").unwrap(),
        );
        let lock_handle = LockHandle::new(first.erase(), "LOCK-HANDLE".to_owned());

        let error = second.unlock(lock_handle).unwrap_err();

        assert!(matches!(
            error,
            ObjectError::LockHandleObjectMismatch { .. }
        ));
    }
}
