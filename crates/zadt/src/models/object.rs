use std::fmt;

use serde::Deserialize;

use crate::{EntityTag, ObjectError, ObjectRef, SourceRef};

/// A fetched source representation and its attached metadata.
#[derive(Debug)]
pub struct SourceCode {
    /// The source resource that was fetched.
    pub reference: SourceRef,

    /// The complete UTF-8 source text.
    pub content: String,

    /// The response entity tag supplied by SAP, when present.
    pub etag: Option<EntityTag>,
}

impl SourceCode {
    pub(crate) fn new(reference: SourceRef, content: String, etag: Option<EntityTag>) -> Self {
        Self {
            reference,
            content,
            etag,
        }
    }
}

/// The access requested when locking an ADT repository object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessMode {
    /// Locks the object for read-only display.
    Show,

    /// Locks the object for modification.
    Modify,
}

/// An opaque lock obtained for one object in a specific SAP user session.
///
/// The handle is bound to both [`ObjectRef`] and [`crate::UserSession`]. A
/// handle string alone is not sufficient to update another resource.
#[derive(Clone, Eq, PartialEq)]
pub struct LockHandle {
    /// The locked object.
    object: ObjectRef,

    /// The opaque handle supplied by SAP.
    handle: String,
}

impl LockHandle {
    pub(crate) fn new(object: ObjectRef, handle: String) -> Self {
        Self { object, handle }
    }

    /// Returns the object this lock belongs to.
    pub fn object(&self) -> &ObjectRef {
        &self.object
    }

    /// Returns the opaque handle supplied by SAP.
    pub fn handle(&self) -> &str {
        &self.handle
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

pub(crate) fn parse_lock_handle(body: &[u8]) -> Result<String, ObjectError> {
    let raw: RawLock = serde_xml_rs::from_reader(body).map_err(ObjectError::InvalidLockResponse)?;
    raw.values
        .data
        .lock_handle
        .filter(|handle| !handle.is_empty())
        .ok_or(ObjectError::MissingLockHandle)
}

#[derive(Deserialize)]
#[serde(rename = "asx:abap")]
struct RawLock {
    #[serde(rename = "asx:values")]
    values: RawLockValues,
}

#[derive(Deserialize)]
struct RawLockValues {
    #[serde(rename = "DATA")]
    data: RawLockData,
}

#[derive(Deserialize)]
struct RawLockData {
    #[serde(rename = "LOCK_HANDLE")]
    lock_handle: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK_XML: &[u8] = include_bytes!("../../tests/fixtures/object-lock.xml");

    #[test]
    fn parses_an_opaque_object_lock_handle() {
        assert_eq!(parse_lock_handle(LOCK_XML).unwrap(), "LOCK-HANDLE-1");
    }
}
