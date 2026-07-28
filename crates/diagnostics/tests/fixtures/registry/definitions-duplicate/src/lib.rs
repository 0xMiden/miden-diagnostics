#![no_std]
#![forbid(unsafe_code)]

diag_runtime::diagnostic_codes! {
    namespace = "registry::alpha";
    catalog = DUPLICATE_DIAGNOSTICS;

    pub E1000 {
        summary: "duplicate alpha primary failure",
        severity: Error,
    }
}

#[inline(never)]
pub fn anchor() -> &'static [&'static diag_runtime::DiagnosticDescriptor] {
    DUPLICATE_DIAGNOSTICS
}
