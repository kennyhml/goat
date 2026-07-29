mod link;
mod refs;
mod relations;

pub use link::{AdtLink, AdtLinkError};
pub use refs::{
    EnhancementImplementationsRef, HtmlSourceRef, ObjectEnhancementOptionsRef, ObjectStateRef,
    ObjectStructureRef, OwnedResourceRef, ParserRef, SourceEnhancementOptionsRef, SourceRef,
    SourceVersionsRef, TextElementsRef,
};
pub use relations::Relations;

pub(crate) use link::{AdvertisedLink, resolve_href};
