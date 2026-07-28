use super::*;

/// Decides whether one semantic diagnostic occurrence makes an outcome fail.
pub trait FailurePolicy {
    fn is_failure(&self, diagnostic: DiagnosticMetadata<'_>) -> bool;
}

/// The default policy: only effective error occurrences fail.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultFailurePolicy;

impl FailurePolicy for DefaultFailurePolicy {
    fn is_failure(&self, diagnostic: DiagnosticMetadata<'_>) -> bool {
        diagnostic.severity == Severity::Error
    }
}

/// A [FailurePolicy] that treats both warnings and errors as failure.
#[derive(Clone, Copy, Debug, Default)]
pub struct WarningsAsErrors;

impl FailurePolicy for WarningsAsErrors {
    fn is_failure(&self, diagnostic: DiagnosticMetadata<'_>) -> bool {
        matches!(diagnostic.severity, Severity::Error | Severity::Warning)
    }
}
