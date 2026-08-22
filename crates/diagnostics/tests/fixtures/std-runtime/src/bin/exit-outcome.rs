use core::num::NonZeroU16;

use miden_diagnostics::{
    DiagnosticCollector, DiagnosticSink, ExitWithOutcome, Outcome, OwnedDiagnostic, Report,
    Severity, TerminalChoice, TerminalPolicy, TerminalWidth, WarningsAsErrors,
};
use std_runtime::{
    MissingSource, PrepareFailure, SimpleDiagnostic, rich_report, rich_session_diagnostic,
};

fn collect(diagnostics: impl IntoIterator<Item = SimpleDiagnostic>) -> Outcome<&'static str, ()> {
    let mut collector = DiagnosticCollector::new();
    for diagnostic in diagnostics {
        collector.add(diagnostic);
    }
    Outcome {
        result: Ok("recovered"),
        diagnostics: collector.finish(),
    }
}

fn main() -> ExitWithOutcome<&'static str> {
    match std::env::args().nth(1).as_deref() {
        Some("empty") => collect([]).into_exit(),
        Some("warning") => collect([SimpleDiagnostic {
            severity: Severity::Warning,
            message: "warning-only",
        }])
        .into_exit(),
        Some("error") => collect([SimpleDiagnostic {
            severity: Severity::Error,
            message: "error-only",
        }])
        .into_exit(),
        Some("warnings-as-errors") => collect([SimpleDiagnostic {
            severity: Severity::Warning,
            message: "warning-is-failure",
        }])
        .into_exit()
        .with_policy(WarningsAsErrors),
        Some("all") => collect([
            SimpleDiagnostic {
                severity: Severity::Hint,
                message: "first-hint",
            },
            SimpleDiagnostic {
                severity: Severity::Error,
                message: "second-error",
            },
            SimpleDiagnostic {
                severity: Severity::Warning,
                message: "third-warning",
            },
            SimpleDiagnostic {
                severity: Severity::Info,
                message: "fourth-info",
            },
        ])
        .into_exit(),
        Some("attached-source") => {
            let mut collector = DiagnosticCollector::new();
            collector.push(rich_report().into_diagnostic());
            Outcome {
                result: Ok::<_, ()>("recovered"),
                diagnostics: collector.finish(),
            }
            .into_exit()
        }
        Some("session-source") => {
            let (diagnostic, sources) = rich_session_diagnostic();
            let mut collector = DiagnosticCollector::new();
            collector.add(diagnostic);
            Outcome {
                result: Ok::<_, ()>("recovered"),
                diagnostics: collector.finish(),
            }
            .into_exit()
            .with_sources(sources)
        }
        Some("missing-source") => {
            let mut collector = DiagnosticCollector::new();
            collector.add(MissingSource);
            Outcome {
                result: Ok::<_, ()>("recovered"),
                diagnostics: collector.finish(),
            }
            .into_exit()
        }
        Some("missing-source-warning") => {
            let mut collector = DiagnosticCollector::new();
            collector.push(
                OwnedDiagnostic::new(MissingSource).with_severity_override(Severity::Warning),
            );
            Outcome {
                result: Ok::<_, ()>("recovered"),
                diagnostics: collector.finish(),
            }
            .into_exit()
        }
        Some("prepare-failure") => {
            let mut collector = DiagnosticCollector::new();
            collector.push(Report::new(PrepareFailure).into_diagnostic());
            Outcome {
                result: Ok::<_, ()>("recovered"),
                diagnostics: collector.finish(),
            }
            .into_exit()
        }
        Some("explicit-terminal") => {
            let mut collector = DiagnosticCollector::new();
            collector.push(rich_report().into_diagnostic());
            Outcome {
                result: Ok::<_, ()>("recovered"),
                diagnostics: collector.finish(),
            }
            .into_exit()
            .with_terminal_policy(TerminalPolicy {
                styled: TerminalChoice::Always,
                unicode: TerminalChoice::Always,
                hyperlinks: TerminalChoice::Always,
                width: TerminalWidth::Fixed(NonZeroU16::new(100).unwrap()),
            })
        }
        other => panic!("unknown exit-outcome mode: {other:?}"),
    }
}
