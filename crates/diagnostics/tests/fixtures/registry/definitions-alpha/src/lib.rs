#![no_std]
#![forbid(unsafe_code)]

pub const EXPLANATION_SENTINEL: &str = include_str!("../explain/E1000.md");

diag_runtime::diagnostic_codes! {
    namespace = "registry::alpha";
    catalog = ALPHA_DIAGNOSTICS;

    pub E1000 {
        summary: "alpha primary failure",
        severity: Error,
        explanation: include_str!("../explain/E1000.md"),
        documentation_url: "https://example.com/registry/alpha/E1000",
    }

    pub E2000 {
        summary: "alpha secondary failure",
        severity: Error,
    }

    pub N4000 {
        summary: "alpha explanation not provided",
        severity: Info,
        documentation_url: "http://example.com/registry/alpha/N4000",
    }
}

/// A definition-only inline descriptor. The evidence never constructs this
/// type or calls its descriptor getter.
#[derive(Debug, diag_runtime::Diagnostic)]
#[diagnostic(
    crate = diag_runtime,
    code = "registry::alpha/I5000",
    summary = "definition-only inline descriptor",
    severity = Hint,
    message = "definition-only inline descriptor"
)]
pub struct DefinitionOnly;

#[inline(never)]
pub fn anchor() -> &'static [&'static diag_runtime::DiagnosticDescriptor] {
    ALPHA_DIAGNOSTICS
}
