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

#[cfg(test)]
mod test {
    use crate::{Class, Include, ObjectError, ObjectType, Program};

    #[test]
    fn object_name_policies_enforce_type_specific_limits() {
        assert_eq!(Program::NAMING_POLICY.maximum_length(), 30);
        assert_eq!(Include::NAMING_POLICY.maximum_length(), 40);
        assert_eq!(Class::NAMING_POLICY.maximum_length(), 30);
        assert!(Program::NAMING_POLICY.validate(&"A".repeat(30)).is_ok());
        assert!(Include::NAMING_POLICY.validate(&"A".repeat(40)).is_ok());

        let name = "A".repeat(31);
        let error = Program::NAMING_POLICY.validate(&name).unwrap_err();
        assert!(matches!(
            error,
            ObjectError::NameTooLong {
                name: rejected,
                maximum_length: 30,
            } if rejected == name
        ));
    }
}
