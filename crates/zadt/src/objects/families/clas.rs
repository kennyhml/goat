use super::super::{
    GlobalWorkbenchType, ObjectCollection, ObjectNamePolicy, ObjectRef, ObjectType, Source,
    SourceComponent, private,
};
use crate::{resource::SourceRef, vocabulary::CategoryId};

/// An ABAP class object.
#[derive(Debug)]
pub enum Class {}

impl private::Sealed for Class {}

impl ObjectType for Class {
    const WORKBENCH_TYPE: GlobalWorkbenchType = GlobalWorkbenchType::new("CLAS", "OC");
    const NAMING_POLICY: ObjectNamePolicy = ObjectNamePolicy::new(30);
    const SOURCE_COMPONENTS: &'static [&'static dyn SourceComponent] = &[
        &ClassSourceComponent::Main,
        &ClassSourceComponent::Definitions,
        &ClassSourceComponent::Implementations,
        &ClassSourceComponent::Macros,
        &ClassSourceComponent::TestClasses,
    ];
}

impl ObjectCollection for Class {
    const CATEGORY: CategoryId = CategoryId {
        scheme: "http://www.sap.com/adt/categories/oo",
        term: "classes",
    };
}

impl Source for Class {}

/// A source component owned and locked by an ABAP class.
///
/// Local class includes are ADT resources beneath the class object rather than
/// independent repository objects.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ClassSourceComponent {
    Main,
    Definitions,
    Implementations,
    Macros,
    TestClasses,
}

impl ClassSourceComponent {
    /// Returns the component name used by ADT.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Definitions => "definitions",
            Self::Implementations => "implementations",
            Self::Macros => "macros",
            Self::TestClasses => "testclasses",
        }
    }
}

impl SourceComponent for ClassSourceComponent {
    fn name(&self) -> &'static str {
        self.as_str()
    }

    fn path(&self) -> &'static [&'static str] {
        match self {
            Self::Main => &["source", "main"],
            Self::Definitions => &["includes", "definitions"],
            Self::Implementations => &["includes", "implementations"],
            Self::Macros => &["includes", "macros"],
            Self::TestClasses => &["includes", "testclasses"],
        }
    }

    fn is_primary(&self) -> bool {
        matches!(self, Self::Main)
    }
}

impl ObjectRef<Class> {
    /// Resolves one of the source resources owned by this class.
    pub fn component_source(&self, component: ClassSourceComponent) -> SourceRef {
        self.source_from_component(&component)
    }

    #[cfg(test)]
    pub(crate) fn for_test(name: &str, uri: crate::AdtUri) -> Self {
        Self::typed(name.to_ascii_uppercase(), uri)
    }
}
