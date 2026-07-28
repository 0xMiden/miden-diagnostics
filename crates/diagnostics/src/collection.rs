mod collector;
mod diagnostic_set;
mod failure_policy;
mod limits;
#[cfg(test)]
mod tests;

use alloc::{boxed::Box, string::String, vec, vec::Vec};
use core::slice;

pub use self::{
    collector::{DiagnosticCollector, MergeSummary},
    diagnostic_set::{DiagnosticEntry, DiagnosticInstanceId, DiagnosticSet, SeverityCounts},
    failure_policy::{DefaultFailurePolicy, FailurePolicy, WarningsAsErrors},
    limits::DiagnosticLimits,
};
use self::{diagnostic_set::DiagnosticMetadataSnapshot, limits::LimitReachedDiagnostic};
use crate::{
    Diagnostic, DiagnosticCodeRef, DiagnosticDescriptor, DiagnosticMetadata, DiagnosticTag,
    OwnedDiagnostic, Report, Severity,
};

/// An [Outcome] represents the output of a computation and the set of diagnostics that were
/// produced during that computation. Diagnostics may or may not indicate failure, so this type
/// does not treat an outcome as either/or - a successful outcome may still produce diagnostics,
/// which is the primary distinction between this type and [`core::result::Result<T, E>`].
#[derive(Debug)]
pub struct Outcome<T> {
    pub value: T,
    pub diagnostics: DiagnosticSet,
}

impl<T> Outcome<T> {
    /// Converts this outcome into the std application's rich termination path.
    #[cfg(feature = "std")]
    pub fn into_exit(self) -> crate::ExitWithOutcome<T> {
        crate::ExitWithOutcome::new(self)
    }

    /// Convert this outcome into a [Result] based on the provided [FailurePolicy].
    pub fn into_result<P>(self, policy: &P) -> Result<Self, Self>
    where
        P: FailurePolicy + ?Sized,
    {
        if self.diagnostics.assess(policy) {
            Err(self)
        } else {
            Ok(self)
        }
    }
}

/// Represents a type-erased diagnostic sink for use in libraries.
///
/// This abstracts over how diagnostics are collected and reported by the parent application.
pub trait DiagnosticSink {
    /// Push `diagnostic` to the sink
    ///
    /// Returns a `PushResult` that indicates whether the diagnostic was successfully recorded, or
    /// if it was rejected due to limits or configuration.
    fn push(&mut self, diagnostic: OwnedDiagnostic) -> PushResult;
}

/// Result of pushing one user diagnostic into a sink.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PushResult {
    /// The diagnostic was accepted successfully
    Accepted,
    /// The diagnostic was dropped due to a limit on diagnostics that match its characteristics
    DroppedByLimit,
    /// The diagnostic was dropped because the receiver was closed after a limit was reached
    ClosedAfterLimit,
}
