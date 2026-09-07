use core::num::NonZeroU32;

#[cfg(feature = "std")]
static NEXT_SOURCE_NAMESPACE: std::sync::Mutex<u32> = std::sync::Mutex::new(u32::MAX);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceRevision(pub u32);

/// A caller-coordinated source identity namespace.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceNamespace(Option<NonZeroU32>);

impl Default for SourceNamespace {
    fn default() -> Self {
        Self::UNKNOWN
    }
}

impl core::fmt::Display for SourceNamespace {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if let Some(value) = self.0 {
            core::fmt::Display::fmt(&value, f)
        } else {
            f.write_str("unknown")
        }
    }
}

impl SourceNamespace {
    /// Represents an unknown/undefined source namespace.
    ///
    /// This namespace can _never_ represent a valid source file - it is only used to represent
    /// the fact that a source reference is invalid/unknown.
    pub const UNKNOWN: Self = Self(None);

    /// Construct a new [SourceNamespace] with the given unique identifier
    pub const fn new(namespace: NonZeroU32) -> Self {
        Self(Some(namespace))
    }

    /// Construct a new [SourceNamespace] with the given unique identifier
    ///
    /// If the input value is 0, this will produce `SourceNamespace::UNKNOWN` which will result
    /// in associated source identifiers to be treated as invalid/unresolvable
    pub fn new_unchecked(namespace: u32) -> Self {
        Self(NonZeroU32::new(namespace))
    }

    /// Returns true if this represents an invalid/unknown source namespace
    pub const fn is_unknown(self) -> bool {
        self.0.is_none()
    }

    /// Allocates a process-local namespace distinct from previous calls to this function.
    ///
    /// Explicitly chosen namespaces remain caller-coordinated and should not use the high,
    /// descending range reserved by this allocator.
    #[cfg(feature = "std")]
    pub fn fresh() -> Option<Self> {
        let mut next =
            NEXT_SOURCE_NAMESPACE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let value = NonZeroU32::new(*next)?;
        *next = value.get().saturating_sub(1);
        Some(Self::new(value))
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn fresh_namespaces_are_known_and_distinct() {
        let first = SourceNamespace::fresh().unwrap();
        let second = SourceNamespace::fresh().unwrap();
        assert!(!first.is_unknown());
        assert!(!second.is_unknown());
        assert_ne!(first, second);
    }
}

#[cfg(feature = "arbitrary")]
impl proptest::prelude::Arbitrary for SourceNamespace {
    type Parameters = ();
    type Strategy = proptest::prelude::BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        use proptest::prelude::*;

        any::<u32>().prop_map(SourceNamespace::new_unchecked).boxed()
    }
}

/// A source identity scoped to a provider/session namespace.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceId {
    namespace: SourceNamespace,
    local: u32,
}

impl Default for SourceId {
    fn default() -> Self {
        Self::UNKNOWN
    }
}

impl SourceId {
    pub const UNKNOWN: Self = Self {
        namespace: SourceNamespace::UNKNOWN,
        local: u32::MAX,
    };

    pub const fn new(namespace: SourceNamespace, local: u32) -> Self {
        Self { namespace, local }
    }

    pub const fn namespace(self) -> SourceNamespace {
        self.namespace
    }

    pub const fn local(self) -> u32 {
        self.local
    }

    pub const fn is_unknown(self) -> bool {
        self.namespace.is_unknown()
    }
}

#[cfg(feature = "arbitrary")]
impl proptest::prelude::Arbitrary for SourceId {
    type Parameters = ();
    type Strategy = proptest::prelude::BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        use proptest::prelude::*;

        (any::<SourceNamespace>(), any::<u32>())
            .prop_map(|(namespace, local)| SourceId { namespace, local })
            .boxed()
    }
}

/// Provenance for a source identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SourceKey {
    Session(SourceId),
    Attached(SourceId),
}

impl SourceKey {
    pub const fn id(self) -> SourceId {
        match self {
            Self::Session(id) | Self::Attached(id) => id,
        }
    }
}
