use crate::ObjectRef;

use super::{AdtLink, AdtLinkError, AdvertisedLink, OwnedResourceRef, refs::FromAdtLink};

/// Maps relations of an object reference and enables lazy evaluation
/// of possible references. While the underlying [`ObjectRef`] could
/// be a statically known [`ObjectRef<T>`], as the relations can only
/// be found by having a reference to such, it should always be used
/// as an implementation detail, only to be called on directly in
/// exceptional cases, such as when access to a relation is not yet
/// supported statically.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Relations {
    owner: ObjectRef,
    links: Box<[AdvertisedLink]>,
}

impl Relations {
    pub(crate) fn new(owner: ObjectRef, links: Vec<AdvertisedLink>) -> Self {
        Self {
            owner,
            links: links.into_boxed_slice(),
        }
    }

    /// Returns the number of advertised links without resolving them.
    pub fn len(&self) -> usize {
        self.links.len()
    }

    /// Returns whether no links were advertised.
    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }

    /// Resolves advertised links in document order.
    pub fn iter<'a>(&'a self) -> impl ExactSizeIterator<Item = Result<AdtLink, AdtLinkError>> + 'a {
        self.links.iter().map(|link| link.resolve(self.owner.uri()))
    }

    /// Resolves and converts the relation associated to `R`.
    ///
    /// The link is resolved against the owner of the relation as the base,
    /// enforcing a valid resource reference.
    pub(crate) fn get<R>(&self) -> Result<Option<OwnedResourceRef<R>>, AdtLinkError>
    where
        R: FromAdtLink,
    {
        self.links
            .iter()
            .find(|link| link.matches(R::RELATION, R::MEDIA_TYPE))
            .map(|link| {
                link.resolve(self.owner.uri())
                    .map(|link| R::from_adt_link(&self.owner, &link))
            })
            .transpose()
    }
}
