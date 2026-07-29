use crate::error::ObjectError;

/// Static naming constraints for an object type.
///
/// For instance, a program or class name may be up to 30 characters long,
/// while a table type only supports 16 characters.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ObjectNamePolicy {
    maximum_length: usize,
}

impl ObjectNamePolicy {
    /// Creates a naming policy with the maximum supported object-name length.
    pub const fn new(maximum_length: usize) -> Self {
        Self { maximum_length }
    }

    /// Returns the maximum number of characters accepted in an object name.
    pub const fn maximum_length(self) -> usize {
        self.maximum_length
    }

    pub(crate) fn validate(self, name: &str) -> Result<(), ObjectError> {
        if name.is_empty()
            || name.trim() != name
            || name.chars().any(char::is_control)
            || matches!(name, "." | "..")
        {
            return Err(ObjectError::InvalidName {
                name: name.to_owned(),
            });
        }
        if name.chars().count() > self.maximum_length {
            return Err(ObjectError::NameTooLong {
                name: name.to_owned(),
                maximum_length: self.maximum_length,
            });
        }
        Ok(())
    }
}
