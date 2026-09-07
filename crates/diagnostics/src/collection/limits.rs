use core::fmt;

use crate::Diagnostic;

/// Optional limits over accepted user diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiagnosticLimits {
    /// A limit on the total number of diagnostics, `None` means no limit
    pub max_diagnostics: Option<usize>,
    /// A limit on the total number of error-severity diagnostics, `None` means no limit
    pub max_errors: Option<usize>,
}

impl DiagnosticLimits {
    pub const UNLIMITED: Self = Self {
        max_diagnostics: None,
        max_errors: None,
    };

    pub const fn new(max_diagnostics: Option<usize>, max_errors: Option<usize>) -> Self {
        Self {
            max_diagnostics,
            max_errors,
        }
    }
}

impl Default for DiagnosticLimits {
    fn default() -> Self {
        Self::UNLIMITED
    }
}

#[derive(Debug)]
pub(super) struct LimitReachedDiagnostic;

impl Diagnostic for LimitReachedDiagnostic {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("too many diagnostics; collection stopped")
    }
}
