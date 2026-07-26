mod discovery;
mod object;
mod programs;

pub(crate) use discovery::parse_capabilities;
pub(crate) use object::parse_lock_handle;
pub(crate) use programs::{parse_include, parse_program};

pub use discovery::{Capabilities, Category, Collection, TemplateLink, Workspace};
pub use object::{AccessMode, LockHandle, SourceCode};
pub use programs::{Include, Program, SyntaxConfiguration, SyntaxLanguage};
