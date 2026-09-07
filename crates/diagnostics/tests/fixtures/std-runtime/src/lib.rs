use core::fmt;

use miden_diagnostics::{
    DescriptorOrigin, Diagnostic, DiagnosticCode, DiagnosticDescriptor, DiagnosticTag, Explanation,
    Label, LabelStyle, Note, NoteKind, Report, Severity, SourceId, SourceKey, SourceMap,
    SourceNamespace, SourceSpan, TextRange, VisitDiagnostic,
};

pub const EXPLANATION_SENTINEL: &str = "PHASE4_LONG_EXPLANATION_MUST_NOT_RENDER";

pub static RICH_DESCRIPTOR: DiagnosticDescriptor = DiagnosticDescriptor {
    code: DiagnosticCode {
        namespace: "std",
        code: "E_REPORT",
    },
    summary: "rich report",
    default_severity: Severity::Error,
    explanation: Explanation::Embedded(EXPLANATION_SENTINEL),
    documentation_url: Some("https://example.com/std/E_REPORT"),
    tags: &[DiagnosticTag::Deprecated],
    origin: DescriptorOrigin {
        module_path: module_path!(),
        file: file!(),
        line: line!(),
    },
};

#[derive(Debug)]
pub struct RichDiagnostic {
    span: SourceSpan,
}

impl Diagnostic for RichDiagnostic {
    fn descriptor(&self) -> Option<&'static DiagnosticDescriptor> {
        Some(&RICH_DESCRIPTOR)
    }

    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("could not compile module")
    }

    fn visit(&self, visitor: &mut dyn VisitDiagnostic) {
        visitor.label(Label {
            span: self.span,
            style: LabelStyle::Primary,
            message: Some(format_args!("unexpected token")),
        });
        visitor.note(Note {
            kind: NoteKind::Help,
            message: format_args!("replace `broken`"),
        });
    }
}

pub fn rich_report() -> Report {
    let mut sources = SourceMap::new(SourceNamespace::new_unchecked(44));
    let source = sources
        .insert("attached.masm", "begin\n    broken\nend\n", None)
        .expect("fixture source must fit");
    let range = TextRange::new(10, 16).expect("fixture range is valid");
    Report::new(RichDiagnostic {
        span: SourceSpan::attached(source, range),
    })
    .context("while lowering component")
    .attach_sources(sources)
}

pub fn rich_session_diagnostic() -> (RichDiagnostic, SourceMap) {
    let mut sources = SourceMap::new(SourceNamespace::new_unchecked(46));
    let source = sources
        .insert("session.masm", "begin\n    broken\nend\n", None)
        .expect("fixture source must fit");
    let range = TextRange::new(10, 16).expect("fixture range is valid");
    (
        RichDiagnostic {
            span: SourceSpan::session(source, range),
        },
        sources,
    )
}

#[derive(Debug)]
pub struct MissingSource;

impl Diagnostic for MissingSource {
    fn descriptor(&self) -> Option<&'static DiagnosticDescriptor> {
        static DESCRIPTOR: DiagnosticDescriptor = DiagnosticDescriptor {
            code: DiagnosticCode {
                namespace: "std",
                code: "E_MISSING",
            },
            summary: "missing source",
            default_severity: Severity::Error,
            explanation: Explanation::NotProvided,
            documentation_url: None,
            tags: &[],
            origin: DescriptorOrigin {
                module_path: module_path!(),
                file: file!(),
                line: line!(),
            },
        };
        Some(&DESCRIPTOR)
    }

    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("source is unavailable")
    }

    fn visit(&self, visitor: &mut dyn VisitDiagnostic) {
        visitor.label(Label {
            span: SourceSpan::new(
                SourceKey::Attached(SourceId::new(SourceNamespace::new_unchecked(45), 0)),
                None,
                TextRange::new(0, 1).unwrap(),
            ),
            style: LabelStyle::Primary,
            message: None,
        });
    }
}

#[derive(Debug)]
pub struct PrepareFailure;

impl Diagnostic for PrepareFailure {
    fn message(&self, _out: &mut dyn fmt::Write) -> fmt::Result {
        Err(fmt::Error)
    }
}

#[derive(Debug)]
pub struct SimpleDiagnostic {
    pub severity: Severity,
    pub message: &'static str,
}

impl Diagnostic for SimpleDiagnostic {
    fn severity(&self) -> Severity {
        self.severity
    }

    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str(self.message)
    }
}
