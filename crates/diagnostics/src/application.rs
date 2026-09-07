use alloc::boxed::Box;
use std::process::{ExitCode, Termination};

use crate::{
    DefaultFailurePolicy, Emitter, FailurePolicy, Outcome, SourceProvider, StderrEmitter,
    TerminalPolicy,
    terminal::{EMISSION_FALLBACK, PREPARATION_FALLBACK, write_stderr_fallback},
};

/// A rich application result that emits every diagnostic before returning.
pub struct ExitWithOutcome<T = ()> {
    outcome: Outcome<T>,
    sources: Option<Box<dyn SourceProvider>>,
    policy: Box<dyn FailurePolicy>,
    terminal_policy: TerminalPolicy,
}

impl<T> ExitWithOutcome<T> {
    pub(crate) fn new(outcome: Outcome<T>) -> Self {
        Self {
            outcome,
            sources: None,
            policy: Box::new(DefaultFailurePolicy),
            terminal_policy: TerminalPolicy::default(),
        }
    }

    /// Overrides the retained session source providers used by default.
    pub fn with_sources<P>(mut self, sources: P) -> Self
    where
        P: SourceProvider + 'static,
    {
        self.sources = Some(Box::new(sources));
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

impl<T> From<Outcome<T>> for ExitWithOutcome<T> {
    fn from(outcome: Outcome<T>) -> Self {
        Self::new(outcome)
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
            sources.as_deref(),
            policy.as_ref(),
            |prepared| emitter.emit_set(prepared).map(|_| ()),
            write_stderr_fallback,
        )
    }
}

fn report_with<T, E>(
    outcome: Outcome<T>,
    sources: Option<&dyn SourceProvider>,
    policy: &dyn FailurePolicy,
    mut emit: impl FnMut(&crate::PreparedSet<'_>) -> Result<(), E>,
    mut fallback: impl FnMut(&[u8]),
) -> ExitCode {
    let policy_failed = outcome.is_err_with_policy(policy);
    let prepared = match sources {
        Some(sources) => outcome.diagnostics.prepare(sources),
        None => outcome.diagnostics.prepare_attached(),
    };
    let prepared = match prepared {
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
    use crate::{Diagnostic, DiagnosticCollector, Severity, source::EmptySourceProvider};

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
            result: Ok(()),
            diagnostics: collector.finish(),
        }
    }

    #[test]
    fn infrastructure_failures_override_policy_success_with_static_fallbacks() {
        let sources = EmptySourceProvider;
        let mut fallback = Vec::new();
        let status = report_with(
            outcome(Simple(Severity::Warning)),
            Some(&sources),
            &DefaultFailurePolicy,
            |_| Err::<(), ()>(()),
            |message| fallback.extend_from_slice(message),
        );
        assert_eq!(status, ExitCode::FAILURE);
        assert_eq!(fallback, EMISSION_FALLBACK);

        fallback.clear();
        let status = report_with(
            outcome(Unpreparable),
            Some(&sources),
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
            Some(&sources),
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

    #[derive(Debug)]
    struct Located(crate::SourceSpan);

    impl Diagnostic for Located {
        fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
            out.write_str("located diagnostic")
        }

        fn visit(&self, visitor: &mut dyn crate::VisitDiagnostic) {
            visitor.label(crate::Label {
                span: self.0,
                style: crate::LabelStyle::Primary,
                message: None,
            });
        }
    }

    #[test]
    fn retained_sources_are_used_unless_explicitly_overridden() {
        use crate::{SourceMap, SourceNamespace, SourceSpan, TextRange};

        let namespace = SourceNamespace::new_unchecked(1);
        for explicit_override in [false, true] {
            let mut retained = SourceMap::new(namespace);
            let id = retained.insert("retained.masm", "retained source", None).unwrap();
            let mut outcome =
                outcome(Located(SourceSpan::session(id, TextRange::new(0, 8).unwrap())));
            outcome.diagnostics =
                outcome.diagnostics.attach_session_sources(alloc::sync::Arc::new(retained));
            let mut exit = ExitWithOutcome::from(outcome);
            if explicit_override {
                let mut sources = SourceMap::new(namespace);
                assert_eq!(sources.insert("override.masm", "override source", None).unwrap(), id);
                exit = exit.with_sources(sources);
            }
            let mut rendered = alloc::string::String::new();
            let status = report_with(
                exit.outcome,
                exit.sources.as_deref(),
                exit.policy.as_ref(),
                |prepared| {
                    rendered = alloc::format!("{prepared}");
                    Ok::<(), ()>(())
                },
                |_| panic!("diagnostic preparation and emission should succeed"),
            );
            assert_eq!(status, ExitCode::FAILURE);
            let expected = if explicit_override {
                "override"
            } else {
                "retained"
            };
            assert!(rendered.contains(&alloc::format!("{expected}.masm")), "{rendered}");
            assert!(rendered.contains(&alloc::format!("{expected} source")), "{rendered}");
            let unexpected = if explicit_override {
                "retained"
            } else {
                "override"
            };
            assert!(!rendered.contains(&alloc::format!("{unexpected}.masm")), "{rendered}");
        }
    }

    #[test]
    fn failed_result_without_diagnostics_returns_failure() {
        let status = report_with(
            Outcome::<()> {
                result: Err(()),
                diagnostics: Default::default(),
            },
            None,
            &DefaultFailurePolicy,
            |_| Ok::<(), ()>(()),
            |_| panic!("empty diagnostics should prepare and emit successfully"),
        );
        assert_eq!(status, ExitCode::FAILURE);
    }

    #[test]
    fn warning_status_respects_the_failure_policy() {
        for (policy, expected) in [
            (&DefaultFailurePolicy as &dyn FailurePolicy, ExitCode::SUCCESS),
            (&crate::WarningsAsErrors as &dyn FailurePolicy, ExitCode::FAILURE),
        ] {
            let mut emitted = 0;
            let status = report_with(
                outcome(Simple(Severity::Warning)),
                None,
                policy,
                |prepared| {
                    emitted = prepared.len();
                    Ok::<(), ()>(())
                },
                |_| panic!("warning should prepare and emit successfully"),
            );
            assert_eq!(emitted, 1);
            assert_eq!(status, expected);
        }
    }
}
