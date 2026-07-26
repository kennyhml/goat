use derive_builder::Builder;
use http::{HeaderValue, Method, StatusCode, header};

use crate::{
    client::{Client, ClientState},
    error::{ObjectError, OperationError, ResponseError},
    models::{AccessMode, LockHandle, SourceCode, parse_lock_handle},
    operation::{Operation, Stateful, Stateless},
    protocol::{AdtRequest, AdtResponse},
    resource::{IncludeRef, ObjectRef, ProgramRef, SourceRef},
    vocabulary::{PostAction, media_type, query_parameter},
};

/// Fetches the source code advertised by a [`SourceRef`].
#[derive(Debug)]
#[readonly::make]
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
        request
            .headers_mut()
            .insert(header::ACCEPT, HeaderValue::from_static(media_type::SOURCE));
        Ok(request)
    }

    fn decode(&self, response: AdtResponse) -> Result<Self::Response, ResponseError> {
        expect_ok(&response)?;
        let etag = response
            .headers()
            .get(header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let content =
            String::from_utf8(response.into_body()).map_err(ObjectError::InvalidSourceEncoding)?;
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

/// Locks a repository object within a [`crate::UserSession`].
///
/// The operation sends `POST` with `_action=LOCK` and the configured
/// `accessMode`. The returned [`LockHandle`] must remain in the same user
/// session as subsequent update or unlock operations.
///
/// TODO: Are these WB operations? ADT kinda defines them like that.
#[derive(Debug)]
#[readonly::make]
pub struct ObjectLock {
    /// The repository object to lock.
    pub object: ObjectRef,

    /// Whether the object is locked for display or modification.
    pub access_mode: AccessMode,
}

impl<S: ClientState> Operation<S> for ObjectLock {
    type Response = LockHandle;
    type Kind = Stateful;

    fn request(&self, _client: &Client<S>) -> Result<AdtRequest, OperationError> {
        let mut request = AdtRequest::new(Method::POST, self.object.uri().clone());
        request.push_query(query_parameter::ACTION, PostAction::Lock.as_str());
        request.push_query(query_parameter::ACCESS_MODE, self.access_mode.as_str());
        request.headers_mut().insert(
            header::ACCEPT,
            HeaderValue::from_static(media_type::LOCK_RESULT),
        );
        Ok(request)
    }

    fn decode(&self, response: AdtResponse) -> Result<Self::Response, ResponseError> {
        expect_ok(&response)?;
        let handle = parse_lock_handle(response.body())?;
        Ok(LockHandle::new(self.object.clone(), handle))
    }
}

/// Releases a [`LockHandle`] within its SAP user session.
/// TODO: Are these WB operations? ADT kinda defines them like that.
#[derive(Debug)]
#[readonly::make]
pub struct ObjectUnlock {
    /// The lock to release.
    pub lock_handle: LockHandle,
}

impl<S: ClientState> Operation<S> for ObjectUnlock {
    type Response = ();
    type Kind = Stateful;

    fn request(&self, _client: &Client<S>) -> Result<AdtRequest, OperationError> {
        let mut request = AdtRequest::new(Method::POST, self.lock_handle.object.uri().clone());
        request.push_query(query_parameter::ACTION, PostAction::Unlock.as_str());
        request.push_query(query_parameter::LOCK_HANDLE, &self.lock_handle.handle);
        Ok(request)
    }

    fn decode(&self, response: AdtResponse) -> Result<Self::Response, ResponseError> {
        expect_ok(&response)
    }
}

/// Replaces the complete source code of an object.
///
/// This operation is stateful and requires a [`LockHandle`] issued for the
/// object being updated. The builder verifies this relationship.
/// TODO: Are these WB operations? ADT kinda defines them like that.
#[derive(Builder, Debug)]
#[builder(setter(into), build_fn(validate = Self::validate))]
#[readonly::make]
pub struct ObjectSourceUpdate {
    /// The source resource whose complete content will be replaced.
    pub source: SourceRef,

    /// A modification lock obtained for the source's owning object.
    pub lock_handle: LockHandle,

    /// The complete replacement source text.
    pub content: String,
}

impl ObjectSourceUpdateBuilder {
    fn validate(&self) -> Result<(), String> {
        if let (Some(source), Some(lock_handle)) = (&self.source, &self.lock_handle)
            && source.object != lock_handle.object
        {
            return Err(format!(
                "lock for `{}` cannot update source `{}`",
                lock_handle.object, source
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
        request.push_query(query_parameter::LOCK_HANDLE, &self.lock_handle.handle);
        request.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(media_type::SOURCE_UPDATE),
        );
        request.set_body(self.content.clone());
        Ok(request)
    }

    fn decode(&self, response: AdtResponse) -> Result<Self::Response, ResponseError> {
        expect_ok(&response)
    }
}

impl LockHandle {
    /// Consumes this handle and creates an operation that removes the lock.
    pub fn remove(self) -> ObjectUnlock {
        ObjectUnlock { lock_handle: self }
    }
}

// It is incovenient for consumers to always construct operations from scratch, so we
// can implement them for the reference types they typically already deal with.

impl ProgramRef {
    /// Creates an object-lock operation for this program.
    pub fn lock(&self, access_mode: AccessMode) -> ObjectLock {
        ObjectLock {
            object: self.object().clone(),
            access_mode,
        }
    }

    /// Creates an operation that releases this program's object lock.
    pub fn unlock(&self, lock_handle: LockHandle) -> Result<ObjectUnlock, ObjectError> {
        if self.object() != &lock_handle.object {
            return Err(ObjectError::LockHandleObjectMismatch {
                expected: self.to_string(),
                actual: lock_handle.object.to_string(),
            });
        }
        Ok(ObjectUnlock { lock_handle })
    }
}

impl IncludeRef {
    /// Creates an object-lock operation for this include.
    pub fn lock(&self, access_mode: AccessMode) -> ObjectLock {
        ObjectLock {
            object: self.object().clone(),
            access_mode,
        }
    }

    /// Creates an operation that releases this include's object lock.
    pub fn unlock(&self, lock_handle: LockHandle) -> Result<ObjectUnlock, ObjectError> {
        if self.object() != &lock_handle.object {
            return Err(ObjectError::LockHandleObjectMismatch {
                expected: self.to_string(),
                actual: lock_handle.object.to_string(),
            });
        }
        Ok(ObjectUnlock { lock_handle })
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

    #[test]
    fn object_operations_do_not_require_discovery() {
        fn accepts_undiscovered<O: Operation<crate::Undiscovered>>() {}

        accepts_undiscovered::<ObjectSourceQuery>();
        accepts_undiscovered::<ObjectLock>();
        accepts_undiscovered::<ObjectUnlock>();
        accepts_undiscovered::<ObjectSourceUpdate>();
    }

    #[test]
    fn update_builder_rejects_a_lock_for_another_object() {
        let first = ProgramRef::for_test(
            crate::AdtUri::parse("/sap/bc/adt/programs/programs/ZFIRST").unwrap(),
        );
        let second = ProgramRef::for_test(
            crate::AdtUri::parse("/sap/bc/adt/programs/programs/ZSECOND").unwrap(),
        );
        let lock_handle = LockHandle::new(first.object().clone(), "LOCK-HANDLE".to_owned());

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
        let first = ProgramRef::for_test(
            crate::AdtUri::parse("/sap/bc/adt/programs/programs/ZFIRST").unwrap(),
        );
        let second = ProgramRef::for_test(
            crate::AdtUri::parse("/sap/bc/adt/programs/programs/ZSECOND").unwrap(),
        );
        let lock_handle = LockHandle::new(first.object().clone(), "LOCK-HANDLE".to_owned());

        let error = second.unlock(lock_handle).unwrap_err();

        assert!(matches!(
            error,
            ObjectError::LockHandleObjectMismatch { .. }
        ));
    }
}
