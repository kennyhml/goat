use std::fmt;

use derive_builder::Builder;
use http::{HeaderValue, Method, StatusCode, header};
use serde::Deserialize;

use crate::{
    AdtRequest, AdtResponse, Client, Discovered, ObjectError, ObjectRef, Operation, OperationError,
    ResponseError, SourceRef, Stateful, Stateless,
};

const LOCK_RESULT_MEDIA_TYPE: &str =
    "application/vnd.sap.as+xml; charset=utf-8; dataname=com.sap.adt.lock.Result2";
const SOURCE_MEDIA_TYPE: &str = "text/plain; charset=utf-8";

/// Operation to fetch the source code of a [`SourceRef`].
///
/// The reference to the source can either be constructed manually
/// or be obtained via relations returnd by the query of an object.
#[derive(Debug)]
#[readonly::make]
pub struct ObjectSourceQuery {
    /// The source resource to fetch.
    pub source: SourceRef,
}

impl Operation<Discovered> for ObjectSourceQuery {
    type Response = SourceCode;
    type Kind = Stateless;

    fn request(&self, _client: &Client<Discovered>) -> Result<AdtRequest, OperationError> {
        let mut request = AdtRequest::new(Method::GET, self.source.uri.clone());
        request
            .headers_mut()
            .insert(header::ACCEPT, HeaderValue::from_static("text/plain"));
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
        Ok(SourceCode {
            reference: self.source.clone(),
            content,
            etag,
        })
    }
}

/// A fetched source representation and its attached metadata
#[derive(Debug)]
#[readonly::make]
pub struct SourceCode {
    /// Returns the source resource that was fetched.
    pub reference: SourceRef,

    /// The complete UTF-8 source text.
    pub content: String,

    /// The response entity tag supplied by SAP, when present.
    pub etag: Option<String>,
}

/// The access requested when locking an ADT repository object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessMode {
    /// Locks the object for read-only display.
    Show,

    /// Locks the object for modification.
    Modify,
}

impl AccessMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Show => "SHOW",
            Self::Modify => "MODIFY",
        }
    }
}

/// Locks a repository object within a [`UserSession`](crate::UserSession).
///
/// The operation sends `POST` with `_action=LOCK` and the configured
/// `accessMode`. The returned [`LockHandle`] must remain in the same user
/// session as subsequent update or unlock operations.
#[derive(Debug)]
#[readonly::make]
pub struct ObjectLock {
    /// The repository object to lock.
    pub object: ObjectRef,

    /// Whether the object is locked for display or modification.
    pub access_mode: AccessMode,
}

impl Operation<Discovered> for ObjectLock {
    type Response = LockHandle;
    type Kind = Stateful;

    fn request(&self, _client: &Client<Discovered>) -> Result<AdtRequest, OperationError> {
        let mut request = AdtRequest::new(Method::POST, self.object.uri().clone());
        request.push_query("_action", "LOCK");
        request.push_query("accessMode", self.access_mode.as_str());
        request.headers_mut().insert(
            header::ACCEPT,
            HeaderValue::from_static(LOCK_RESULT_MEDIA_TYPE),
        );
        Ok(request)
    }

    fn decode(&self, response: AdtResponse) -> Result<Self::Response, ResponseError> {
        expect_ok(&response)?;
        let document: LockDocument =
            serde_xml_rs::from_reader(response.body()).map_err(ObjectError::InvalidLockResponse)?;
        let handle = document
            .values
            .data
            .lock_handle
            .filter(|handle| !handle.is_empty())
            .ok_or(ObjectError::MissingLockHandle)?;
        Ok(LockHandle {
            object: self.object.clone(),
            handle,
        })
    }
}

/// An opaque lock obtained for one object in a specific SAP user session.
///
/// The handle is bound to both [`ObjectRef`] and [`UserSession`](crate::UserSession).
/// A handle string alone is not sufficient to update another resource.
#[derive(Clone, Eq, PartialEq)]
#[readonly::make]
pub struct LockHandle {
    /// The locked object.
    pub object: ObjectRef,

    /// The opaque handle supplied by SAP.
    pub handle: String,
}

impl LockHandle {
    /// Consumes this handle and creates an operation that removes the lock.
    ///
    /// This is equivalent to calling [`ObjectRef::unlock`] with the lock's own
    /// object and cannot produce an object mismatch.
    pub fn remove(self) -> ObjectUnlock {
        ObjectUnlock { lock_handle: self }
    }
}

impl fmt::Debug for LockHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LockHandle")
            .field("object", &self.object)
            .field("handle", &"<opaque>")
            .finish()
    }
}

/// Releases a [`LockHandle`] within its SAP user session.
#[derive(Debug)]
#[readonly::make]
pub struct ObjectUnlock {
    /// The lock to release.
    pub lock_handle: LockHandle,
}

impl Operation<Discovered> for ObjectUnlock {
    type Response = ();
    type Kind = Stateful;

    fn request(&self, _client: &Client<Discovered>) -> Result<AdtRequest, OperationError> {
        let mut request = AdtRequest::new(Method::POST, self.lock_handle.object.uri().clone());
        request.push_query("_action", "UNLOCK");
        request.push_query("lockHandle", &self.lock_handle.handle);
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
///
/// Because this operation applies to multiple object types, no single backend
/// handler implements every request.
///
/// SAP dispatches the operation to the handler registered for the resource
/// identified by [`SourceRef`], for example:
///
/// ```text
/// PUT /sap/bc/adt/programs/programs/zprog/source/main
/// PUT /sap/bc/adt/ddic/structures/zstruct/source/main
/// PUT /sap/bc/adt/oo/classes/zcl_class/includes/testclasses
/// ```
///
/// Notably, even when an include of a composite object is being modified,
/// such as a class include, the lock handle is still obtained via the parent
/// and remains valid for its children.
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

impl Operation<Discovered> for ObjectSourceUpdate {
    type Response = ();
    type Kind = Stateful;

    fn request(&self, _client: &Client<Discovered>) -> Result<AdtRequest, OperationError> {
        let mut request = AdtRequest::new(Method::PUT, self.source.uri.clone());
        request.push_query("lockHandle", &self.lock_handle.handle);
        request.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(SOURCE_MEDIA_TYPE),
        );
        request.set_body(self.content.clone());
        Ok(request)
    }

    fn decode(&self, response: AdtResponse) -> Result<Self::Response, ResponseError> {
        expect_ok(&response)
    }
}

impl ObjectRef {
    /// Creates an operation that locks this object with the requested access.
    pub fn lock(&self, access_mode: AccessMode) -> ObjectLock {
        ObjectLock {
            object: self.clone(),
            access_mode,
        }
    }

    /// Creates an operation that releases a lock obtained for this object.
    ///
    /// Returns an error without sending a request if the lock belongs to a
    /// different object. The operation must execute through the same
    /// [`UserSession`](crate::UserSession) that obtained the lock.
    pub fn unlock(&self, lock_handle: LockHandle) -> Result<ObjectUnlock, ObjectError> {
        if self != &lock_handle.object {
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

#[derive(Deserialize)]
#[serde(rename = "asx:abap")]
struct LockDocument {
    #[serde(rename = "asx:values")]
    values: LockValues,
}

#[derive(Deserialize)]
struct LockValues {
    #[serde(rename = "DATA")]
    data: LockData,
}

#[derive(Deserialize)]
struct LockData {
    #[serde(rename = "LOCK_HANDLE")]
    lock_handle: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK_XML: &[u8] = include_bytes!("../tests/fixtures/object-lock.xml");

    #[test]
    fn parses_an_opaque_object_lock_handle() {
        let document: LockDocument = serde_xml_rs::from_reader(LOCK_XML).unwrap();

        assert_eq!(
            document.values.data.lock_handle.as_deref(),
            Some("LOCK-HANDLE-1")
        );
    }

    #[test]
    fn update_builder_rejects_a_lock_for_another_object() {
        let first = ObjectRef::parse("/sap/bc/adt/programs/programs/ZFIRST").unwrap();
        let second = ObjectRef::parse("/sap/bc/adt/programs/programs/ZSECOND").unwrap();
        let lock_handle = LockHandle {
            object: first,
            handle: "LOCK-HANDLE".to_owned(),
        };

        let error = second
            .main_source()
            .update()
            .lock_handle(lock_handle)
            .content("REPORT zsecond.")
            .build()
            .unwrap_err();

        assert!(error.to_string().contains("cannot update source"));
    }

    #[test]
    fn object_rejects_another_objects_lock_for_unlock() {
        let first = ObjectRef::parse("/sap/bc/adt/programs/programs/ZFIRST").unwrap();
        let second = ObjectRef::parse("/sap/bc/adt/programs/programs/ZSECOND").unwrap();
        let lock_handle = LockHandle {
            object: first,
            handle: "LOCK-HANDLE".to_owned(),
        };

        let error = second.unlock(lock_handle).unwrap_err();

        assert!(matches!(
            error,
            ObjectError::LockHandleObjectMismatch { .. }
        ));
    }
}
