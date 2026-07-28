#[cfg(any(test, feature = "linked-registry"))]
use core::cmp::Ordering;
use core::{error::Error, fmt};

use crate::{DescriptorOrigin, DiagnosticCode, DiagnosticDescriptor};

/// A deterministic view over explicitly composed diagnostic catalogs.
///
/// Explicit composition is the portable registry mechanism. Iteration
/// preserves catalog order and descriptor order within each catalog.
#[derive(Clone, Copy, Debug)]
pub struct StaticRegistry<'a> {
    catalogs: &'a [&'a [&'static DiagnosticDescriptor]],
}

impl<'a> StaticRegistry<'a> {
    /// Compose a registry from explicit descriptor catalogs.
    pub const fn from_slices(
        catalogs: &'a [&'a [&'static DiagnosticDescriptor]],
    ) -> StaticRegistry<'a> {
        Self { catalogs }
    }

    /// Iterate over descriptors in explicit catalog order.
    pub fn iter(&self) -> impl Iterator<Item = &'static DiagnosticDescriptor> + '_ {
        self.catalogs.iter().flat_map(|catalog| catalog.iter().copied())
    }

    /// Validate that every canonical code occurs exactly once.
    ///
    /// Duplicate descriptor pointers are rejected just like distinct
    /// descriptors with the same code.
    pub fn validate(&self) -> Result<(), RegistryError> {
        for (index, first) in self.iter().enumerate() {
            for second in self.iter().skip(index + 1) {
                if first.code == second.code {
                    return Err(RegistryError::DuplicateCode {
                        code: first.code,
                        first_origin: first.origin,
                        second_origin: second.origin,
                    });
                }
            }
        }
        Ok(())
    }

    /// Look up a canonical code or a unique unqualified code.
    ///
    /// A string containing `/` is treated as canonical. An unqualified code
    /// succeeds only when it is unique across the complete registry.
    pub fn lookup(&self, code: &str) -> Result<&'static DiagnosticDescriptor, LookupError> {
        self.validate().map_err(LookupError::InvalidRegistry)?;
        lookup_descriptors(self.iter(), code)
    }
}

/// Failure to construct or validate a diagnostic registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistryError {
    /// Two definitions use the same canonical diagnostic code.
    DuplicateCode {
        code: DiagnosticCode,
        first_origin: DescriptorOrigin,
        second_origin: DescriptorOrigin,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateCode {
                code,
                first_origin,
                second_origin,
            } => write!(
                formatter,
                "duplicate diagnostic code {code} at {}:{}:{} and {}:{}:{}",
                first_origin.file,
                first_origin.line,
                first_origin.module_path,
                second_origin.file,
                second_origin.line,
                second_origin.module_path,
            ),
        }
    }
}

impl Error for RegistryError {}

/// Failure to resolve a diagnostic code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LookupError {
    /// No descriptor has the requested canonical or unqualified code.
    UnknownCode,
    /// An unqualified code matches more than one canonical code.
    AmbiguousCode {
        first: DiagnosticCode,
        second: DiagnosticCode,
    },
    /// The selected explicit registry contains duplicate canonical codes.
    InvalidRegistry(RegistryError),
}

impl fmt::Display for LookupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCode => formatter.write_str("unknown diagnostic code"),
            Self::AmbiguousCode { first, second } => {
                write!(formatter, "ambiguous diagnostic code; matches {first} and {second}")
            }
            Self::InvalidRegistry(error) => error.fmt(formatter),
        }
    }
}

impl Error for LookupError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRegistry(error) => Some(error),
            Self::UnknownCode | Self::AmbiguousCode { .. } => None,
        }
    }
}

impl From<RegistryError> for LookupError {
    fn from(error: RegistryError) -> Self {
        Self::InvalidRegistry(error)
    }
}

fn lookup_descriptors(
    descriptors: impl IntoIterator<Item = &'static DiagnosticDescriptor>,
    code: &str,
) -> Result<&'static DiagnosticDescriptor, LookupError> {
    let canonical = match code.split_once('/') {
        Some((namespace, unqualified))
            if !namespace.is_empty() && !unqualified.is_empty() && !unqualified.contains('/') =>
        {
            Some((namespace, unqualified))
        }
        Some(_) => return Err(LookupError::UnknownCode),
        None => None,
    };
    let mut matches = descriptors.into_iter().filter(|descriptor| {
        if let Some((namespace, unqualified)) = canonical {
            descriptor.code.namespace == namespace && descriptor.code.code == unqualified
        } else {
            descriptor.code.code == code
        }
    });

    let Some(first) = matches.next() else {
        return Err(LookupError::UnknownCode);
    };
    let Some(second) = matches.next() else {
        return Ok(first);
    };
    Err(LookupError::AmbiguousCode {
        first: first.code,
        second: second.code,
    })
}

#[cfg(feature = "linked-registry")]
mod linked {
    use alloc::{boxed::Box, vec::Vec};
    use std::sync::OnceLock;

    use super::{
        DiagnosticDescriptor, LookupError, RegistryError, canonical_code_cmp, lookup_descriptors,
        origin_cmp,
    };

    /// A deterministic index of descriptors contributed by linked crates.
    #[derive(Debug)]
    pub struct RegistryIndex {
        descriptors: Box<[&'static DiagnosticDescriptor]>,
    }

    impl RegistryIndex {
        /// Return the number of linked diagnostic definitions.
        pub fn len(&self) -> usize {
            self.descriptors.len()
        }

        /// Return true when no linked diagnostic definitions were retained.
        pub fn is_empty(&self) -> bool {
            self.descriptors.is_empty()
        }

        /// Iterate in canonical-code and definition-origin order.
        pub fn iter(
            &self,
        ) -> impl DoubleEndedIterator<Item = &'static DiagnosticDescriptor> + ExactSizeIterator + '_
        {
            self.descriptors.iter().copied()
        }

        /// Look up a canonical code or a unique unqualified code.
        pub fn lookup(&self, code: &str) -> Result<&'static DiagnosticDescriptor, LookupError> {
            lookup_descriptors(self.iter(), code)
        }
    }

    #[doc(hidden)]
    pub struct RegistryEntry {
        descriptor: &'static DiagnosticDescriptor,
    }

    impl RegistryEntry {
        #[doc(hidden)]
        pub const fn new(descriptor: &'static DiagnosticDescriptor) -> Self {
            Self { descriptor }
        }
    }

    inventory::collect!(RegistryEntry);

    static LINKED_REGISTRY: OnceLock<Result<RegistryIndex, RegistryError>> = OnceLock::new();

    /// Build and return the process-wide linked diagnostic registry.
    ///
    /// Applications embedding raw `wasm32-unknown-unknown` modules must run
    /// their guarded constructor wrapper before the first call.
    pub fn linked_registry() -> Result<&'static RegistryIndex, &'static RegistryError> {
        LINKED_REGISTRY.get_or_init(build_index).as_ref()
    }

    fn build_index() -> Result<RegistryIndex, RegistryError> {
        let mut descriptors: Vec<_> = inventory::iter::<RegistryEntry>
            .into_iter()
            .map(|entry| entry.descriptor)
            .collect();
        descriptors.sort_unstable_by(|left, right| {
            canonical_code_cmp(left.code, right.code)
                .then_with(|| origin_cmp(left.origin, right.origin))
        });

        for pair in descriptors.windows(2) {
            let [first, second] = pair else {
                unreachable!("windows(2) always contains two descriptors");
            };
            if first.code == second.code {
                return Err(RegistryError::DuplicateCode {
                    code: first.code,
                    first_origin: first.origin,
                    second_origin: second.origin,
                });
            }
        }

        Ok(RegistryIndex {
            descriptors: descriptors.into_boxed_slice(),
        })
    }
}

#[cfg(feature = "linked-registry")]
pub use linked::{RegistryEntry, RegistryIndex, linked_registry};

#[cfg(any(test, feature = "linked-registry"))]
fn canonical_code_cmp(left: DiagnosticCode, right: DiagnosticCode) -> Ordering {
    left.namespace
        .bytes()
        .chain(core::iter::once(b'/'))
        .chain(left.code.bytes())
        .cmp(right.namespace.bytes().chain(core::iter::once(b'/')).chain(right.code.bytes()))
}

#[cfg(feature = "linked-registry")]
fn origin_cmp(left: DescriptorOrigin, right: DescriptorOrigin) -> Ordering {
    left.module_path
        .cmp(right.module_path)
        .then_with(|| left.file.cmp(right.file))
        .then_with(|| left.line.cmp(&right.line))
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;
    use core::assert_matches;

    use super::*;
    use crate::{DiagnosticTag, Explanation, Severity};

    static TAGS: &[DiagnosticTag] = &[];

    static ALPHA_E1000: DiagnosticDescriptor = descriptor("alpha", "E1000", 10);
    static ALPHA_W2000: DiagnosticDescriptor = descriptor("alpha", "W2000", 20);
    static BETA_E1000: DiagnosticDescriptor = descriptor("beta", "E1000", 30);
    static ALPHA: &[&DiagnosticDescriptor] = &[&ALPHA_E1000, &ALPHA_W2000];
    static BETA: &[&DiagnosticDescriptor] = &[&BETA_E1000];

    const fn descriptor(
        namespace: &'static str,
        code: &'static str,
        line: u32,
    ) -> DiagnosticDescriptor {
        DiagnosticDescriptor {
            code: DiagnosticCode { namespace, code },
            summary: "summary",
            default_severity: Severity::Error,
            explanation: Explanation::NotProvided,
            documentation_url: None,
            tags: TAGS,
            origin: DescriptorOrigin {
                module_path: "registry::tests",
                file: "registry.rs",
                line,
            },
        }
    }

    #[test]
    fn explicit_iteration_and_lookup_are_repeatable() {
        let catalogs: &[&[&DiagnosticDescriptor]] = &[ALPHA, BETA];
        let registry = StaticRegistry::from_slices(catalogs);

        for _ in 0..2 {
            assert_eq!(
                registry.iter().map(|descriptor| descriptor.code).collect::<Vec<_>>(),
                [ALPHA_E1000.code, ALPHA_W2000.code, BETA_E1000.code]
            );
            assert_eq!(registry.lookup("alpha/E1000"), Ok(&ALPHA_E1000));
            assert_eq!(registry.lookup("W2000"), Ok(&ALPHA_W2000));
            assert_eq!(registry.lookup("/E1000"), Err(LookupError::UnknownCode));
            assert_eq!(registry.lookup("alpha/"), Err(LookupError::UnknownCode));
            assert_eq!(registry.lookup("alpha/E1000/extra"), Err(LookupError::UnknownCode));
            assert_eq!(registry.lookup("missing"), Err(LookupError::UnknownCode));
            assert_eq!(
                registry.lookup("E1000"),
                Err(LookupError::AmbiguousCode {
                    first: ALPHA_E1000.code,
                    second: BETA_E1000.code,
                })
            );
        }
    }

    #[test]
    fn duplicate_code_reports_both_origins_in_composition_order() {
        static DUPLICATE: DiagnosticDescriptor = descriptor("alpha", "E1000", 40);
        let catalog: &[&DiagnosticDescriptor] = &[&ALPHA_E1000, &DUPLICATE];
        let catalogs: &[&[&DiagnosticDescriptor]] = &[catalog];
        let registry = StaticRegistry::from_slices(catalogs);
        let expected = RegistryError::DuplicateCode {
            code: ALPHA_E1000.code,
            first_origin: ALPHA_E1000.origin,
            second_origin: DUPLICATE.origin,
        };

        assert_eq!(registry.validate(), Err(expected));
        assert_eq!(registry.lookup("alpha/E1000"), Err(LookupError::InvalidRegistry(expected)));
    }

    #[test]
    fn repeated_descriptor_pointer_is_a_duplicate() {
        let catalog: &[&DiagnosticDescriptor] = &[&ALPHA_E1000, &ALPHA_E1000];
        let catalogs: &[&[&DiagnosticDescriptor]] = &[catalog];
        assert_matches!(
            StaticRegistry::from_slices(catalogs).validate(),
            Err(RegistryError::DuplicateCode { .. })
        );
    }

    #[test]
    fn canonical_order_compares_the_separator_not_namespace_tuples() {
        let prefix = DiagnosticCode {
            namespace: "a",
            code: "z",
        };
        let longer = DiagnosticCode {
            namespace: "a!",
            code: "a",
        };
        assert_eq!(canonical_code_cmp(longer, prefix), Ordering::Less);
    }
}
