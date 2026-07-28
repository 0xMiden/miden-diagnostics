use alloc::boxed::Box;
use std::process::{ExitCode, Termination};

use crate::{
    DefaultFailurePolicy, Emitter, FailurePolicy, Outcome, SourceProvider, StderrEmitter,
    TerminalPolicy,
    source::EmptySourceProvider,
    terminal::{EMISSION_FALLBACK, PREPARATION_FALLBACK, write_stderr_fallback},
};

/// A rich application result that emits every diagnostic before returning.
pub struct ExitWithOutcome<T = ()> {
    outcome: Outcome<T>,
    sources: Box<dyn SourceProvider>,
    policy: Box<dyn FailurePolicy>,
    terminal_policy: TerminalPolicy,
}

impl<T> ExitWithOutcome<T> {
    pub(crate) fn new(outcome: Outcome<T>) -> Self {
        Self {
            outcome,
            sources: Box::new(EmptySourceProvider),
            policy: Box::new(DefaultFailurePolicy),
            terminal_policy: TerminalPolicy::default(),
        }
    }

    /// Replaces the empty session source provider used by default.
    pub fn with_sources<P>(mut self, sources: P) -> Self
    where
        P: SourceProvider + 'static,
    {
        self.sources = Box::new(sources);
        self
    }

    /// Replaces the default error-only failure policy.
    pub fn with_policy<P>(mut self, policy: P) -> Self
    where
        P: FailurePolicy + 'static,
    {
        self.policy = Box::new(policy);
        self
    }

    /// Selects how stderr terminal capabilities are resolved.
    pub fn with_terminal_policy(mut self, terminal_policy: TerminalPolicy) -> Self {
        self.terminal_policy = terminal_policy;
        self
    }

    pub const fn outcome(&self) -> &Outcome<T> {
        &self.outcome
    }

    pub fn into_outcome(self) -> Outcome<T> {
        self.outcome
    }
}

impl<T> Termination for ExitWithOutcome<T> {
    fn report(self) -> ExitCode {
        let Self {
            outcome,
            sources,
            policy,
            terminal_policy,
        } = self;
        let mut emitter = StderrEmitter::new(terminal_policy);
        report_with(
            outcome,
            sources.as_ref(),
            policy.as_ref(),
            |prepared| emitter.emit_set(prepared).map(|_| ()),
            write_stderr_fallback,
        )
    }
}

fn report_with<T, E>(
    outcome: Outcome<T>,
    sources: &dyn SourceProvider,
    policy: &dyn FailurePolicy,
    mut emit: impl FnMut(&crate::PreparedSet<'_>) -> Result<(), E>,
    mut fallback: impl FnMut(&[u8]),
) -> ExitCode {
    let policy_failed = outcome.diagnostics.assess(policy);
    let prepared = match outcome.diagnostics.prepare(sources) {
        Ok(prepared) => prepared,
        Err(_) => {
            fallback(PREPARATION_FALLBACK);
            return ExitCode::FAILURE;
        }
    };
    if emit(&prepared).is_err() {
        fallback(EMISSION_FALLBACK);
        return ExitCode::FAILURE;
    }
    if policy_failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;
    use core::fmt;

    use super::*;
    use crate::{Diagnostic, DiagnosticCollector, Severity};

    #[derive(Debug)]
    struct Simple(Severity);

    impl Diagnostic for Simple {
        fn severity(&self) -> Severity {
            self.0
        }

        fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
            out.write_str("simple")
        }
    }

    #[derive(Debug)]
    struct Unpreparable;

    impl Diagnostic for Unpreparable {
        fn message(&self, _out: &mut dyn fmt::Write) -> fmt::Result {
            Err(fmt::Error)
        }
    }

    fn outcome(diagnostic: impl Diagnostic + Send + Sync + 'static) -> Outcome<()> {
        let mut collector = DiagnosticCollector::new();
        collector.add(diagnostic);
        Outcome {
            value: (),
            diagnostics: collector.finish(),
        }
    }

    #[test]
    fn infrastructure_failures_override_policy_success_with_static_fallbacks() {
        let sources = EmptySourceProvider;
        let mut fallback = Vec::new();
        let status = report_with(
            outcome(Simple(Severity::Warning)),
            &sources,
            &DefaultFailurePolicy,
            |_| Err::<(), ()>(()),
            |message| fallback.extend_from_slice(message),
        );
        assert_eq!(status, ExitCode::FAILURE);
        assert_eq!(fallback, EMISSION_FALLBACK);

        fallback.clear();
        let status = report_with(
            outcome(Unpreparable),
            &sources,
            &DefaultFailurePolicy,
            |_| Ok::<(), ()>(()),
            |message| fallback.extend_from_slice(message),
        );
        assert_eq!(status, ExitCode::FAILURE);
        assert_eq!(fallback, PREPARATION_FALLBACK);
    }

    #[test]
    fn policy_status_is_used_only_after_successful_infrastructure() {
        let sources = EmptySourceProvider;
        let mut emitted = 0;
        let mut fallback_called = false;
        let status = report_with(
            outcome(Simple(Severity::Error)),
            &sources,
            &DefaultFailurePolicy,
            |prepared| {
                emitted = prepared.len();
                Ok::<(), ()>(())
            },
            |_| fallback_called = true,
        );
        assert_eq!(status, ExitCode::FAILURE);
        assert_eq!(emitted, 1);
        assert!(!fallback_called);
    }
}
