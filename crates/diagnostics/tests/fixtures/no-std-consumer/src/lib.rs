#![no_std]

extern crate alloc;

use core::fmt;

use miden_diagnostics::{
    DefaultFailurePolicy, DescriptorOrigin, Diagnostic, DiagnosticCode, DiagnosticCollector,
    DiagnosticDescriptor, Explanation, LookupError, Outcome, RegistryError, Report, Severity,
    SourceMap, SourceNamespace, SourceProvider, SourceRevision, StaticRegistry,
};

miden_diagnostics::diagnostic_codes! {
    namespace = "portable";
    catalog = PORTABLE_DIAGNOSTICS;

    pub E1000 {
        summary: "portable registry error",
        severity: Error,
        explanation: "portable explanation sentinel",
    }

    pub W2000 {
        summary: "portable registry warning",
        severity: Warning,
    }
}

static SECOND_E1000: DiagnosticDescriptor = DiagnosticDescriptor {
    code: DiagnosticCode {
        namespace: "second",
        code: "E1000",
    },
    summary: "second namespace",
    default_severity: Severity::Error,
    explanation: Explanation::Embedded("embedded fixture sentinel"),
    documentation_url: None,
    tags: &[],
    origin: DescriptorOrigin {
        module_path: "no_std_consumer",
        file: file!(),
        line: line!(),
    },
};
static SECOND_DIAGNOSTICS: &[&DiagnosticDescriptor] = &[&SECOND_E1000];
static PORTABLE_CATALOGS: &[&[&DiagnosticDescriptor]] = &[PORTABLE_DIAGNOSTICS, SECOND_DIAGNOSTICS];
static PORTABLE_REGISTRY: StaticRegistry<'static> = StaticRegistry::from_slices(PORTABLE_CATALOGS);

#[derive(Debug, miden_diagnostics::Diagnostic)]
#[diagnostic(
    code = "portable/W1",
    summary = "portable warning",
    severity = Warning,
    message = "portable warning"
)]
struct Warning;

#[derive(Debug)]
struct Fatal;

impl Diagnostic for Fatal {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("portable failure")
    }
}

pub fn exercise() -> usize {
    let mut sources = SourceMap::new(SourceNamespace(42));
    let source = sources
        .insert("portable.masm", "warn\n", Some(SourceRevision(1)))
        .expect("small fixture source must fit");
    assert_eq!(sources.get(source).unwrap().text, Some("warn\n"));
    assert_eq!(
        sources
            .line_column(source, 5)
            .map(|location| { (location.line(), location.column()) }),
        Some((2, 1))
    );

    let mut warnings = DiagnosticCollector::new();
    warnings.add(Warning);
    let warnings = warnings.finish();
    assert!(!warnings.assess(&DefaultFailurePolicy));

    let warning_outcome = Outcome {
        value: 7_usize,
        diagnostics: warnings,
    };
    let warning_outcome = warning_outcome
        .into_result(&DefaultFailurePolicy)
        .expect("warnings succeed under the default policy");
    assert_eq!(warning_outcome.value, 7);

    let mut failures = DiagnosticCollector::new();
    assert_eq!(failures.capture::<()>(Err(Report::new(Fatal))), None);
    let failures = failures.finish();
    assert!(failures.assess(&DefaultFailurePolicy));

    let derived_code_bytes = Warning
        .descriptor()
        .expect("derived inline descriptor")
        .code
        .code
        .len();
    5 + derived_code_bytes + failures.counts().errors()
}

/// Return one bit for every portable explicit-registry invariant.
///
/// This deliberately avoids assertions so a failed probe returns evidence
/// instead of entering the no-std panic handler's spin loop.
pub fn registry_probe() -> i32 {
    let mut bits = 0_i32;

    let ordered = PORTABLE_REGISTRY
        .iter()
        .map(|descriptor| descriptor.code)
        .eq([E1000.code, W2000.code, SECOND_E1000.code]);
    if ordered && PORTABLE_REGISTRY.validate().is_ok() {
        bits |= 1 << 0;
    }
    if matches!(
        PORTABLE_REGISTRY.lookup("portable/E1000"),
        Ok(descriptor) if core::ptr::eq(descriptor, &E1000)
    ) {
        bits |= 1 << 1;
    }
    if matches!(
        PORTABLE_REGISTRY.lookup("W2000"),
        Ok(descriptor) if core::ptr::eq(descriptor, &W2000)
    ) {
        bits |= 1 << 2;
    }
    if matches!(
        PORTABLE_REGISTRY.lookup("E1000"),
        Err(LookupError::AmbiguousCode {
            first,
            second,
        }) if first == E1000.code && second == SECOND_E1000.code
    ) {
        bits |= 1 << 3;
    }
    if matches!(
        PORTABLE_REGISTRY.lookup("missing"),
        Err(LookupError::UnknownCode)
    ) && matches!(
        PORTABLE_REGISTRY.lookup("portable/E1000/extra"),
        Err(LookupError::UnknownCode)
    ) {
        bits |= 1 << 4;
    }

    let duplicate_catalog: &[&DiagnosticDescriptor] = &[&E1000, &E1000];
    let duplicate_catalogs: &[&[&DiagnosticDescriptor]] = &[duplicate_catalog];
    if matches!(
        StaticRegistry::from_slices(duplicate_catalogs).validate(),
        Err(RegistryError::DuplicateCode {
            code,
            first_origin,
            second_origin,
        }) if code == E1000.code
            && first_origin == E1000.origin
            && second_origin == E1000.origin
    ) {
        bits |= 1 << 5;
    }

    if E1000.explanation == Explanation::NotEmbedded
        && W2000.explanation == Explanation::NotProvided
        && SECOND_E1000.explanation == Explanation::Embedded("embedded fixture sentinel")
    {
        bits |= 1 << 6;
    }
    if PORTABLE_REGISTRY.iter().count() == 3
        && PORTABLE_REGISTRY.iter().count() == 3
        && matches!(
            PORTABLE_REGISTRY.lookup("W2000"),
            Ok(descriptor) if core::ptr::eq(descriptor, &W2000)
        )
    {
        bits |= 1 << 7;
    }

    bits
}

#[allow(dead_code)]
pub fn panic_with_report(report: Report) -> ! {
    miden_diagnostics::panic_report!(report)
}
