mod collector;
mod diagnostic_set;
mod failure_policy;
mod limits;
#[cfg(test)]
mod tests;

use alloc::{boxed::Box, string::String, sync::Arc, vec, vec::Vec};
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
    OwnedDiagnostic, Report, Severity, SourceProvider,
};

/// An [Outcome] represents the output of a computation and the set of diagnostics that were
/// produced during that computation. Diagnostics may or may not indicate failure, so this type
/// does not treat an outcome as either/or - a successful outcome may still produce diagnostics,
/// which is the primary distinction between this type and [`core::result::Result<T, E>`].
#[derive(Debug)]
pub struct Outcome<T, E = ()> {
    pub result: Result<T, E>,
    pub diagnostics: DiagnosticSet,
}

impl<T, E> Outcome<T, E> {
    /// Returns true if this outcome represents success according to the default failure policy
    #[inline]
    pub fn is_ok(&self) -> bool {
        self.is_ok_with_policy(&DefaultFailurePolicy)
    }

    /// Returns true if this outcome represents success according to `policy`
    pub fn is_ok_with_policy<P>(&self, policy: &P) -> bool
    where
        P: ?Sized + FailurePolicy,
    {
        self.result.is_ok() && !self.diagnostics.assess(policy)
    }

    /// Returns true if this outcome represents failure according to the default failure policy
    #[inline]
    pub fn is_err(&self) -> bool {
        self.is_err_with_policy(&DefaultFailurePolicy)
    }

    /// Returns true if this outcome represents failure according to `policy`
    #[inline]
    pub fn is_err_with_policy<P>(&self, policy: &P) -> bool
    where
        P: ?Sized + FailurePolicy,
    {
        self.result.is_err() || self.diagnostics.assess(policy)
    }

    /// Transform the value associated with this outcome, preserving the diagnostics
    pub fn map<U>(self, mapper: impl FnOnce(T) -> U) -> Outcome<U, E> {
        Outcome {
            result: self.result.map(mapper),
            diagnostics: self.diagnostics,
        }
    }

    /// Transform the error value associated with this outcome, preserving the diagnostics
    pub fn map_err<U>(self, mapper: impl FnOnce(E) -> U) -> Outcome<T, U> {
        Outcome {
            result: self.result.map_err(mapper),
            diagnostics: self.diagnostics,
        }
    }

    /// Transform the value with a fallible associated with this outcome, preserving the diagnostics
    pub fn and_then<U>(
        self,
        mapper: impl FnOnce(T, &mut DiagnosticCollector) -> Result<U, E>,
    ) -> Outcome<U, E> {
        let mut collector = DiagnosticCollector::default();
        collector.merge(self.diagnostics);
        let result = self.result.and_then(|value| mapper(value, &mut collector));
        Outcome {
            result,
            diagnostics: collector.finish(),
        }
    }
}

impl<T, E> Outcome<T, E>
where
    Report: From<E>,
{
    /// Unwrap a successful outcome/value of type `T`, or panic.
    #[track_caller]
    pub fn unwrap(self) -> T {
        self.into_result().unwrap()
    }

    /// Expect this outcome to have successfully produced a value of `T`, or panic with `message`
    ///
    /// Returns the `T` that was produced, and discards the diagnostics.
    #[track_caller]
    pub fn expect(self, message: &str) -> T {
        self.into_result().expect(message)
    }

    /// Expect this outcome to have failed to produce a value of `T`, or panic with `message`
    ///
    /// Returns the diagnostics associated with this outcome
    #[track_caller]
    pub fn expect_err(self, message: &str) -> DiagnosticSet {
        if self.is_ok() {
            panic!("{message}");
        }
        match self.result {
            Ok(_) => self.diagnostics,
            Err(err) => {
                let mut collector = DiagnosticCollector::default();
                collector.add_report(Report::from(err));
                collector.merge(self.diagnostics);
                collector.finish()
            }
        }
    }

    /// Convert this outcome into a `Result<T, Report>` using the default failure policy
    ///
    /// If the inner `Result` of this outcome is an error, it will be converted to a `Report` as
    /// the primary diagnostic
    pub fn into_result(self) -> Result<T, Report> {
        self.into_result_with_policy(&DefaultFailurePolicy)
    }

    /// Convert this outcome into a `Result<T, Report>` using the provided failure policy
    pub fn into_result_with_policy<P>(self, policy: &P) -> Result<T, Report>
    where
        P: ?Sized + FailurePolicy,
    {
        if self.is_ok_with_policy(policy) {
            self.result.map_err(Report::from)
        } else {
            match self.result {
                Ok(_) => Err(self.diagnostics.into_report(policy)),
                Err(err) => {
                    let mut collector = DiagnosticCollector::default();
                    collector.add_report(Report::from(err));
                    collector.merge(self.diagnostics);
                    let diagnostics = collector.finish();
                    Err(diagnostics.into_report(policy))
                }
            }
        }
    }

    /// Converts this outcome into the std application's rich termination path.
    #[cfg(feature = "std")]
    pub fn into_exit(self) -> crate::ExitWithOutcome<T> {
        let Self {
            result,
            diagnostics,
        } = self;
        let outcome = match result {
            Ok(value) => Outcome {
                result: Ok(value),
                diagnostics,
            },
            Err(err) => {
                let mut collector = DiagnosticCollector::default();
                collector.add_report(Report::from(err));
                collector.merge(diagnostics);
                Outcome {
                    result: Err(()),
                    diagnostics: collector.finish(),
                }
            }
        };
        crate::ExitWithOutcome::new(outcome)
    }
}

impl<T> Outcome<T, ()> {
    pub fn from_report(report: Report) -> Self {
        let mut collector = DiagnosticCollector::default();
        collector.add_report(report);
        Self {
            result: Err(()),
            diagnostics: collector.finish(),
        }
    }

    /// Unwrap a successful outcome/value of type `T`, or panic.
    #[track_caller]
    pub fn unwrap(self) -> T {
        self.into_result().unwrap()
    }

    /// Expect this outcome to have successfully produced a value of `T`, or panic with `message`
    ///
    /// Returns the `T` that was produced, and discards the diagnostics.
    #[track_caller]
    pub fn expect(self, message: &str) -> T {
        self.into_result().expect(message)
    }

    /// Expect this outcome to have failed to produce a value of `T`, or panic with `message`
    ///
    /// Returns the diagnostics associated with this outcome
    #[track_caller]
    pub fn expect_err(self, message: &str) -> DiagnosticSet {
        if self.is_ok() {
            panic!("{message}");
        }
        self.diagnostics
    }

    /// Convert this outcome into a `Result<T, Report>` using the default failure policy
    ///
    /// If the inner `Result` of this outcome is an error, it will be converted to a `Report` as
    /// the primary diagnostic
    pub fn into_result(self) -> Result<T, Report> {
        self.into_result_with_policy(&DefaultFailurePolicy)
    }

    /// Convert this outcome into a `Result<T, Report>` using the provided failure policy
    pub fn into_result_with_policy<P>(self, policy: &P) -> Result<T, Report>
    where
        P: ?Sized + FailurePolicy,
    {
        if self.is_ok_with_policy(policy) {
            self.result.map_err(|_| Report::msg("operation failed with no diagnostics"))
        } else {
            Err(self.diagnostics.into_report(policy))
        }
    }

    /// Converts this outcome into the std application's rich termination path.
    #[cfg(feature = "std")]
    pub fn into_exit(self) -> crate::ExitWithOutcome<T> {
        crate::ExitWithOutcome::new(self)
    }
}

impl<T, E> From<Result<T, E>> for Outcome<T, E> {
    fn from(result: Result<T, E>) -> Self {
        Self {
            result,
            diagnostics: DiagnosticSet::default(),
        }
    }
}

impl<T> From<T> for Outcome<T> {
    fn from(value: T) -> Self {
        Self {
            result: Ok(value),
            diagnostics: DiagnosticSet::default(),
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
