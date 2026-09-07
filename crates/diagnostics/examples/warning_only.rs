use core::fmt;

use miden_diagnostics::{DefaultFailurePolicy, Diagnostic, DiagnosticCollector, Severity};

#[derive(Debug)]
struct Warning;

impl Diagnostic for Warning {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("the input is accepted with a warning")
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }
}

fn main() {
    let mut diagnostics = DiagnosticCollector::new();
    diagnostics.add(Warning);
    let diagnostics = diagnostics.finish();

    assert_eq!(diagnostics.counts().warnings(), 1);
    assert!(!diagnostics.assess(&DefaultFailurePolicy));
}
