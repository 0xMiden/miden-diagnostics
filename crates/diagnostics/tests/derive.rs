extern crate alloc;

use alloc::{boxed::Box, format, rc::Rc, string::String, vec, vec::Vec};
use core::{cell::Cell, error::Error, fmt};

use diag::{
    AnnotateRenderer, Applicability, Diagnostic as _, DiagnosticCodeRef, DiagnosticTag,
    Explanation, LabelStyle, LayeredSourceProvider, OwnedLabel, OwnedSuggestion, OwnedTextEdit,
    PreparedDiagnostic, Severity, SourceId, SourceMap, SourceNamespace, SourceSpan, Spanned,
    TextRange, prepare_ref,
};
use miden_diagnostics as diag;

diag::diagnostic_codes! {
    namespace = "derive";
    catalog = PHASE3_DIAGNOSTICS;

    pub E_FULL {
        summary: "full derived diagnostic",
        severity: Error,
        documentation_url: "https://example.com/derive/E_FULL",
        tags: [Deprecated],
    }

    pub W_SHARED {
        summary: "shared warning",
        severity: Warning,
    }
}

fn span(start: u32, end: u32) -> SourceSpan {
    SourceSpan::session(
        SourceId::new(SourceNamespace::new_unchecked(3), 0),
        TextRange::new(start, end).unwrap(),
    )
}

#[derive(Debug)]
struct Cause(&'static str);

impl fmt::Display for Cause {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for Cause {}

#[derive(Debug, diag::Diagnostic)]
#[diagnostic(
    crate = diag,
    code = "derive/related",
    summary = "related diagnostic",
    severity = Info,
    message = "related {value}"
)]
struct Related {
    value: u8,
}

#[derive(Debug)]
struct DomainSpan(SourceSpan);

impl Spanned for DomainSpan {
    fn span(&self) -> SourceSpan {
        self.0
    }
}

#[derive(Debug, diag::Diagnostic)]
#[diagnostic(crate = diag, message = "custom spanned field")]
struct CustomSpannedDiagnostic {
    #[label(primary, "domain label")]
    label: DomainSpan,
    #[suggestion(
        "replace domain value",
        replacement = "fixed",
        applicability = MachineApplicable
    )]
    suggestion: DomainSpan,
}

#[derive(Debug, diag::Diagnostic)]
#[diagnostic(
    crate = diag,
    descriptor = E_FULL,
    message = "expected {expected}, found {found}",
    help = "replace {found} with {expected}"
)]
struct FullDiagnostic {
    expected: &'static str,
    found: &'static str,
    #[label(primary, "unexpected {found}")]
    primary: SourceSpan,
    #[label("opened here")]
    opened: Option<SourceSpan>,
    #[labels]
    dynamic_labels: Vec<OwnedLabel>,
    #[note]
    note: String,
    #[help]
    extra_help: Vec<String>,
    #[suggestion(
        "insert {expected}",
        replacement = "{expected}",
        applicability = MachineApplicable
    )]
    insertion: SourceSpan,
    #[suggestions]
    dynamic_suggestions: Vec<OwnedSuggestion>,
    #[related]
    related: Vec<Related>,
    #[source]
    cause: Option<Cause>,
    #[diagnostic_source]
    rich_source: Related,
}

#[derive(Debug, diag::Diagnostic)]
#[diagnostic(crate = diag, transparent)]
struct Transparent(#[diagnostic_source] Related);

#[derive(Debug, diag::Diagnostic)]
#[diagnostic(crate = diag, forward(inner))]
struct Forward {
    ignored: u8,
    #[diagnostic_source]
    inner: Related,
}

#[derive(Debug, diag::Diagnostic)]
#[diagnostic(crate = diag, message = "boxed source")]
struct BoxedDiagnosticSource {
    #[diagnostic_source]
    source: Box<Related>,
}

#[derive(Debug, diag::Diagnostic)]
#[diagnostic(crate = diag, message = "optional boxed source")]
struct OptionalBoxedDiagnosticSource {
    #[diagnostic_source]
    source: Option<Box<Related>>,
}

#[derive(Debug, diag::Diagnostic)]
#[diagnostic(crate = diag)]
enum EnumDiagnostic {
    #[diagnostic(
        descriptor = W_SHARED,
        message = "enum named {value}"
    )]
    Named { value: u8 },
    #[diagnostic(
        code = "derive/enum-hint",
        summary = "enum hint",
        severity = Hint,
        explanation = "enum explanation",
        tags = [Unnecessary],
        message = "enum unit"
    )]
    Unit,
    #[diagnostic(transparent)]
    Related(#[diagnostic_source] Related),
}

#[derive(Debug, diag::Diagnostic)]
#[diagnostic(crate = diag)]
struct DisplayFallback;

impl fmt::Display for DisplayFallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("display fallback")
    }
}

#[derive(Debug, diag::Diagnostic)]
#[diagnostic(crate = diag, message = "generic {value}")]
struct Generic<T: fmt::Display + fmt::Debug> {
    value: T,
}

#[test]
fn descriptor_declarations_and_derive_cover_the_complete_protocol() {
    assert_eq!(PHASE3_DIAGNOSTICS.len(), 2);
    assert!(core::ptr::eq(PHASE3_DIAGNOSTICS[0], &E_FULL));
    assert!(core::ptr::eq(PHASE3_DIAGNOSTICS[1], &W_SHARED));
    assert_eq!(E_FULL.code.to_string(), "derive/E_FULL");
    assert_eq!(E_FULL.tags, &[DiagnosticTag::Deprecated]);

    let diagnostic = FullDiagnostic {
        expected: ")",
        found: "]",
        primary: span(0, 1),
        opened: Some(span(2, 3)),
        dynamic_labels: vec![OwnedLabel {
            span: span(4, 5),
            style: LabelStyle::Context,
            message: Some("dynamic label".into()),
        }],
        note: "field note".into(),
        extra_help: vec!["first help".into(), "second help".into()],
        insertion: span(1, 1),
        dynamic_suggestions: vec![OwnedSuggestion {
            message: "swap both".into(),
            applicability: Applicability::MaybeIncorrect,
            edits: vec![
                OwnedTextEdit {
                    span: span(0, 1),
                    replacement: "(".into(),
                },
                OwnedTextEdit {
                    span: span(2, 3),
                    replacement: ")".into(),
                },
            ],
        }],
        related: vec![Related { value: 7 }],
        cause: Some(Cause("plain cause")),
        rich_source: Related { value: 8 },
    };

    let snapshot = prepare_ref(&diagnostic).unwrap();
    assert!(core::ptr::eq(snapshot.descriptor.unwrap(), &E_FULL));
    assert_eq!(snapshot.message, "expected ), found ]");
    assert_eq!(snapshot.severity, Severity::Error);
    assert_eq!(snapshot.labels.len(), 3);
    assert_eq!(snapshot.notes.len(), 4);
    assert_eq!(snapshot.suggestions.len(), 2);
    assert_eq!(snapshot.suggestions[0].edits[0].replacement, ")");
    assert_eq!(snapshot.suggestions[1].edits.len(), 2);
    assert_eq!(snapshot.related[0].message, "related 7");
    assert_eq!(snapshot.causes[0].message, "plain cause");
    assert_eq!(snapshot.diagnostic_source.as_ref().unwrap().message, "related 8");

    let mut sources = SourceMap::new(SourceNamespace::new_unchecked(3));
    assert_eq!(
        sources.insert("derive.masm", "][abc", None).unwrap(),
        SourceId::new(SourceNamespace::new_unchecked(3), 0)
    );
    let prepared = PreparedDiagnostic {
        snapshot,
        sources: LayeredSourceProvider::new(&sources, None),
    };
    let rendered = AnnotateRenderer::default().render(&prepared).unwrap();
    assert!(rendered.contains("expected ), found ]"));
    assert!(rendered.contains("swap both"));
}

#[test]
fn enums_generics_display_fallback_and_forwarding_preserve_semantics() {
    let named = EnumDiagnostic::Named { value: 4 };
    assert_eq!(named.to_string_for_test(), "enum named 4");
    assert_eq!(named.severity(), Severity::Warning);
    assert!(core::ptr::eq(named.descriptor().unwrap(), &W_SHARED));

    let unit = EnumDiagnostic::Unit;
    assert_eq!(unit.to_string_for_test(), "enum unit");
    assert_eq!(unit.severity(), Severity::Hint);
    assert_eq!(unit.tags(), &[DiagnosticTag::Unnecessary]);
    #[cfg(feature = "embed-explanations")]
    assert_eq!(
        unit.descriptor().unwrap().explanation,
        Explanation::Embedded("enum explanation")
    );
    #[cfg(not(feature = "embed-explanations"))]
    assert_eq!(unit.descriptor().unwrap().explanation, Explanation::NotEmbedded);

    let related = EnumDiagnostic::Related(Related { value: 9 });
    assert_eq!(related.to_string_for_test(), "related 9");
    assert_eq!(related.severity(), Severity::Info);

    assert_eq!(Transparent(Related { value: 3 }).to_string_for_test(), "related 3");
    let forward = Forward {
        ignored: 1,
        inner: Related { value: 5 },
    };
    assert_eq!(forward.ignored, 1);
    assert_eq!(forward.to_string_for_test(), "related 5");
    assert_eq!(forward.severity(), Severity::Info);

    let boxed = BoxedDiagnosticSource {
        source: Box::new(Related { value: 13 }),
    };
    assert_eq!(diagnostic_message(boxed.diagnostic_source().unwrap()), "related 13");
    let optional_boxed = OptionalBoxedDiagnosticSource {
        source: Some(Box::new(Related { value: 14 })),
    };
    assert_eq!(diagnostic_message(optional_boxed.diagnostic_source().unwrap()), "related 14");

    assert_eq!(DisplayFallback.to_string_for_test(), "display fallback");
    assert_eq!(Generic { value: 11 }.to_string_for_test(), "generic 11");
}

#[test]
fn singular_labels_and_suggestions_accept_custom_spanned_fields() {
    let diagnostic = CustomSpannedDiagnostic {
        label: DomainSpan(span(0, 1)),
        suggestion: DomainSpan(span(2, 3)),
    };
    let snapshot = prepare_ref(&diagnostic).unwrap();
    assert_eq!(snapshot.labels[0].span, span(0, 1));
    assert_eq!(snapshot.suggestions[0].edits[0].span, span(2, 3));
}

trait DiagnosticTestExt: diag::Diagnostic {
    fn to_string_for_test(&self) -> String
    where
        Self: Sized,
    {
        diagnostic_message(self)
    }
}

impl<T: diag::Diagnostic> DiagnosticTestExt for T {}

fn diagnostic_message(diagnostic: &dyn diag::Diagnostic) -> String {
    struct Message<'a>(&'a dyn diag::Diagnostic);
    impl fmt::Display for Message<'_> {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.message(formatter)
        }
    }
    format!("{}", Message(diagnostic))
}

#[test]
fn expression_macros_format_immediately_and_never_change_descriptor_identity() {
    let name = String::from("binding");
    let diagnostic = diag::diagnostic! {
        severity: Info,
        code: "plugin/runtime-check",
        message: "`{name}` needs attention",
        labels: [
            primary(span(0, 1), "new `{name}`"),
            context(span(2, 3), "old `{name}`"),
        ],
        notes: [
            note("a note for `{name}`"),
            help("rename `{name}`"),
        ],
        suggestions: [
            suggestion(
                "replace `{name}` twice",
                applicability: MachineApplicable,
                edits: [
                    edit(span(0, 1), "first-{name}"),
                    edit(span(2, 3), "second-{name}"),
                ]
            ),
        ],
        cause: Cause("macro cause"),
        diagnostic_source: Related { value: 6 },
        related: [Related { value: 7 }],
    };
    drop(name);

    let snapshot = prepare_ref(&diagnostic).unwrap();
    assert_eq!(
        snapshot.code.as_ref().map(|code| code.as_ref()),
        Some(DiagnosticCodeRef {
            namespace: "plugin",
            code: "runtime-check",
        })
    );
    assert_eq!(snapshot.severity, Severity::Info);
    assert_eq!(snapshot.message, "`binding` needs attention");
    assert_eq!(snapshot.suggestions[0].edits.len(), 2);
    assert_eq!(snapshot.suggestions[0].edits[1].replacement, "second-binding");
    assert_eq!(snapshot.causes[0].message, "macro cause");

    let descriptor_diagnostic = diag::diagnostic! {
        descriptor: &W_SHARED,
        message: "descriptor occurrence",
    };
    assert_eq!(descriptor_diagnostic.severity(), Severity::Warning);
    assert!(core::ptr::eq(descriptor_diagnostic.descriptor().unwrap(), &W_SHARED));

    let report = diag::report! {
        descriptor: &W_SHARED,
        message: "promoted warning",
    };
    assert_eq!(report.severity(), Severity::Error);
    assert!(core::ptr::eq(report.descriptor().unwrap(), &W_SHARED));
    assert_eq!(W_SHARED.default_severity, Severity::Warning);

    let typed = diag::report!(Related { value: 12 });
    assert_eq!(typed.severity(), Severity::Error);
}

#[test]
fn expression_diagnostics_cover_all_severities_and_erase_borrowed_formatting() {
    struct LocalDisplay<'a>(&'a Cell<u8>);

    impl fmt::Display for LocalDisplay<'_> {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.get().fmt(formatter)
        }
    }

    fn require_transport<T: Send + Sync + 'static>(_: &T) {}

    let local = Rc::new(Cell::new(9));
    let borrowed = diag::diagnostic! {
        severity: Hint,
        message: (LocalDisplay(&local)),
    };
    drop(local);
    require_transport(&borrowed);
    assert_eq!(borrowed.to_string_for_test(), "9");

    let diagnostics = [
        diag::diagnostic! { severity: Error, message: "error" },
        diag::diagnostic! { severity: Warning, message: "warning" },
        diag::diagnostic! { severity: Info, message: "info" },
        diag::diagnostic! { severity: Hint, message: "hint" },
    ];
    assert_eq!(
        diagnostics.map(|diagnostic| diagnostic.severity()),
        [Severity::Error, Severity::Warning, Severity::Info, Severity::Hint,]
    );

    let overridden = diag::diagnostic! {
        descriptor: &W_SHARED,
        severity: Hint,
        message: "overridden",
    };
    assert_eq!(overridden.severity(), Severity::Hint);
    assert_eq!(W_SHARED.default_severity, Severity::Warning);
}
