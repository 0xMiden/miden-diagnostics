#![no_std]
#![forbid(unsafe_code)]

renamed_diagnostics::diagnostic_codes! {
    namespace = "registry::beta";
    catalog = BETA_DIAGNOSTICS;

    pub E1000 {
        summary: "beta shares alpha's short code",
        severity: Error,
    }

    pub W3000 {
        summary: "beta warning",
        severity: Warning,
    }
}

#[inline(never)]
pub fn anchor() -> &'static [&'static renamed_diagnostics::DiagnosticDescriptor] {
    BETA_DIAGNOSTICS
}
