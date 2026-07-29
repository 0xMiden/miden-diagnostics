use core::{
    cell::Cell,
    error::Error,
    fmt::{self, Write as _},
    sync::atomic::{AtomicBool, Ordering},
};
use std::{rc::Rc, string::String, sync::Arc, vec, vec::Vec};

use miden_diagnostics::{
    AnnotateRenderer, Applicability, DescriptorOrigin, Diagnostic, DiagnosticCode,
    DiagnosticCodeOwned, DiagnosticCollector, DiagnosticDescriptor, DiagnosticRelation,
    DiagnosticSnapshot, DiagnosticTag, Emitter, Explanation, FmtEmitter, Label, LabelStyle,
    LayeredSourceProvider, LineColumn, Note, NoteKind, OwnedCause, OwnedDiagnostic, OwnedLabel,
    OwnedNote, OwnedSuggestion, OwnedTextEdit, PreparationItemKind, PreparationLimits,
    PrepareError, PreparedDiagnostic, RenderConfig, RenderError, Severity, Source, SourceId,
    SourceKey, SourceMap, SourceNamespace, SourceProvider, SourceRevision, SourceSpan, Suggestion,
    TextEdit, TextRange, VisitDiagnostic, WrapErr, prepare_ref, prepare_ref_with_limits,
};
#[cfg(feature = "std")]
use miden_diagnostics::{EmissionStatus, IoEmissionError, IoEmitter};

static TAGS: &[DiagnosticTag] = &[DiagnosticTag::Deprecated];
static DESCRIPTOR: DiagnosticDescriptor = DiagnosticDescriptor {
    code: DiagnosticCode {
        namespace: "miden::parser",
        code: "E0007",
    },
    summary: "test descriptor",
    default_severity: Severity::Error,
    explanation: Explanation::Embedded("EXPLANATION_SENTINEL_MUST_NOT_RENDER"),
    documentation_url: Some("https://docs.example.test/E0007"),
    tags: TAGS,
    origin: DescriptorOrigin {
        module_path: "render",
        file: "render.rs",
        line: 1,
    },
};
static INVALID_URL_DESCRIPTOR: DiagnosticDescriptor = DiagnosticDescriptor {
    code: DiagnosticCode {
        namespace: "miden::parser",
        code: "E0008",
    },
    summary: "bad URL",
    default_severity: Severity::Error,
    explanation: Explanation::NotProvided,
    documentation_url: Some("javascript:alert(1)"),
    tags: &[],
    origin: DescriptorOrigin {
        module_path: "render",
        file: "render.rs",
        line: 2,
    },
};

fn range(start: u32, end: u32) -> TextRange {
    TextRange::new(start, end).unwrap()
}

#[derive(Debug)]
struct CellDisplay<'a>(&'a Cell<u32>);

impl fmt::Display for CellDisplay<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.get().fmt(formatter)
    }
}

#[derive(Debug)]
struct LocalDiagnostic<'a> {
    prefix: &'a str,
    value: Rc<Cell<u32>>,
    span: SourceSpan,
}

impl Diagnostic for LocalDiagnostic<'_> {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        write!(out, "{} {}", self.prefix, self.value.get())
    }

    fn visit(&self, visitor: &mut dyn VisitDiagnostic) {
        visitor.label(Label {
            span: self.span,
            style: LabelStyle::Context,
            message: Some(format_args!("label {}", CellDisplay(&self.value))),
        });
        self.value.set(8);
        visitor.note(Note {
            kind: NoteKind::Note,
            message: format_args!("note {}", CellDisplay(&self.value)),
        });
        self.value.set(9);
        let edits = [TextEdit {
            span: self.span,
            replacement: format_args!("replacement {}", CellDisplay(&self.value)),
        }];
        visitor.suggestion(Suggestion {
            message: format_args!("suggestion {}", CellDisplay(&self.value)),
            applicability: Applicability::MachineApplicable,
            edits: &edits,
        });
        self.value.set(10);
    }
}

#[test]
fn standalone_preparation_formats_borrowed_values_immediately_and_promotes_primary() {
    let id = SourceId::new(SourceNamespace(1), 0);
    let value = Rc::new(Cell::new(7));
    let prefix = String::from("local");
    let diagnostic = LocalDiagnostic {
        prefix: &prefix,
        value: Rc::clone(&value),
        span: SourceSpan::session(id, range(0, 0)),
    };

    let snapshot = prepare_ref(&diagnostic).unwrap();
    value.set(99);

    assert_eq!(snapshot.instance_id, None);
    assert_eq!(snapshot.message, "local 7");
    assert_eq!(snapshot.labels[0].style, LabelStyle::Primary);
    assert_eq!(snapshot.labels[0].message.as_deref(), Some("label 7"));
    assert_eq!(snapshot.notes[0].message, "note 8");
    assert_eq!(snapshot.suggestions[0].message, "suggestion 9");
    assert_eq!(snapshot.suggestions[0].edits[0].replacement, "replacement 9");
}

#[derive(Debug)]
struct FormattingFailure;

impl Diagnostic for FormattingFailure {
    fn message(&self, _out: &mut dyn fmt::Write) -> fmt::Result {
        Err(fmt::Error)
    }
}

#[derive(Debug)]
struct BadDisplay;

impl fmt::Display for BadDisplay {
    fn fmt(&self, _formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        Err(fmt::Error)
    }
}

#[derive(Debug)]
struct VisitorFormattingFailure {
    span: SourceSpan,
}

#[derive(Clone, Copy, Debug)]
enum VisitorFormattingSite {
    Label,
    Suggestion,
    Replacement,
}

#[derive(Debug)]
struct VisitorFormattingAt {
    site: VisitorFormattingSite,
    span: SourceSpan,
}

impl Diagnostic for VisitorFormattingAt {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("root")
    }

    fn visit(&self, visitor: &mut dyn VisitDiagnostic) {
        match self.site {
            VisitorFormattingSite::Label => visitor.label(Label {
                span: self.span,
                style: LabelStyle::Primary,
                message: Some(format_args!("{BadDisplay}")),
            }),
            VisitorFormattingSite::Suggestion => {
                let edits = [TextEdit {
                    span: self.span,
                    replacement: format_args!("replacement"),
                }];
                visitor.suggestion(Suggestion {
                    message: format_args!("{BadDisplay}"),
                    applicability: Applicability::Unspecified,
                    edits: &edits,
                });
            }
            VisitorFormattingSite::Replacement => {
                let edits = [TextEdit {
                    span: self.span,
                    replacement: format_args!("{BadDisplay}"),
                }];
                visitor.suggestion(Suggestion {
                    message: format_args!("suggestion"),
                    applicability: Applicability::Unspecified,
                    edits: &edits,
                });
            }
        }
    }
}

#[derive(Debug)]
struct BadCauseDisplay;

impl fmt::Display for BadCauseDisplay {
    fn fmt(&self, _formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        Err(fmt::Error)
    }
}

impl Error for BadCauseDisplay {}

static BAD_CAUSE_DISPLAY: BadCauseDisplay = BadCauseDisplay;

#[derive(Debug)]
struct CauseFormattingFailure;

impl Diagnostic for CauseFormattingFailure {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("root")
    }

    fn cause(&self) -> Option<&(dyn Error + 'static)> {
        Some(&BAD_CAUSE_DISPLAY)
    }
}

impl Diagnostic for VisitorFormattingFailure {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("root")
    }

    fn visit(&self, visitor: &mut dyn VisitDiagnostic) {
        visitor.note(Note {
            kind: NoteKind::Help,
            message: format_args!("{BadDisplay}"),
        });
        visitor.label(Label {
            span: self.span,
            style: LabelStyle::Primary,
            message: None,
        });
    }
}

#[derive(Debug)]
struct MultiplePrimaries {
    span: SourceSpan,
}

impl Diagnostic for MultiplePrimaries {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("multiple")
    }

    fn visit(&self, visitor: &mut dyn VisitDiagnostic) {
        for _ in 0..2 {
            visitor.label(Label {
                span: self.span,
                style: LabelStyle::Primary,
                message: None,
            });
        }
    }
}

#[derive(Debug)]
struct EmptySuggestion;

impl Diagnostic for EmptySuggestion {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("empty")
    }

    fn visit(&self, visitor: &mut dyn VisitDiagnostic) {
        visitor.suggestion(Suggestion {
            message: format_args!("empty"),
            applicability: Applicability::Unspecified,
            edits: &[],
        });
    }
}

#[test]
fn preparation_reports_formatting_primary_and_empty_suggestion_errors() {
    let span = SourceSpan::session(SourceId::new(SourceNamespace(2), 0), range(0, 0));
    assert_eq!(prepare_ref(&FormattingFailure), Err(PrepareError::MessageFormatting));
    assert_eq!(
        prepare_ref(&VisitorFormattingFailure { span }),
        Err(PrepareError::VisitorFormatting {
            item: PreparationItemKind::NoteMessage
        })
    );
    assert_eq!(
        prepare_ref(&MultiplePrimaries { span }),
        Err(PrepareError::MultiplePrimaryLabels { count: 2 })
    );
    assert_eq!(prepare_ref(&EmptySuggestion), Err(PrepareError::EmptySuggestion { index: 0 }));
    for (site, item) in [
        (VisitorFormattingSite::Label, PreparationItemKind::LabelMessage),
        (VisitorFormattingSite::Suggestion, PreparationItemKind::SuggestionMessage),
        (VisitorFormattingSite::Replacement, PreparationItemKind::Replacement),
    ] {
        assert_eq!(
            prepare_ref(&VisitorFormattingAt { site, span }),
            Err(PrepareError::VisitorFormatting { item })
        );
    }
    assert_eq!(
        prepare_ref(&CauseFormattingFailure),
        Err(PrepareError::CauseFormatting { depth: 1 })
    );
}

#[derive(Debug)]
struct SelfCycle;

impl Diagnostic for SelfCycle {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("cycle")
    }

    fn visit(&self, visitor: &mut dyn VisitDiagnostic) {
        visitor.related(self);
    }
}

#[derive(Debug)]
struct SourceCycle;

impl Diagnostic for SourceCycle {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("source cycle")
    }

    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        Some(self)
    }
}

#[derive(Debug)]
struct SourceChain(Option<Box<SourceChain>>);

impl Diagnostic for SourceChain {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("source chain")
    }

    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        self.0.as_deref().map(|source| source as &dyn Diagnostic)
    }
}

#[derive(Debug)]
struct Chain(Option<Box<Chain>>);

impl Diagnostic for Chain {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("chain")
    }

    fn visit(&self, visitor: &mut dyn VisitDiagnostic) {
        if let Some(next) = self.0.as_deref() {
            visitor.related(next);
        }
    }
}

#[test]
fn preparation_has_conservative_cycle_detection_and_authoritative_limits() {
    assert_eq!(
        prepare_ref(&SelfCycle),
        Err(PrepareError::DiagnosticCycle {
            relation: DiagnosticRelation::Related,
            depth: 1,
        })
    );
    assert_eq!(
        prepare_ref(&SourceCycle),
        Err(PrepareError::DiagnosticCycle {
            relation: DiagnosticRelation::DiagnosticSource,
            depth: 1,
        })
    );

    let source_chain = SourceChain(Some(Box::new(SourceChain(None))));
    let source_limits = PreparationLimits {
        max_diagnostic_source_depth: 0,
        ..PreparationLimits::default()
    };
    assert_eq!(
        prepare_ref_with_limits(&source_chain, source_limits),
        Err(PrepareError::DiagnosticSourceDepthExceeded { depth: 1, limit: 0 })
    );

    let chain = Chain(Some(Box::new(Chain(None))));
    let limits = PreparationLimits {
        max_related_depth: 0,
        ..PreparationLimits::default()
    };
    assert_eq!(
        prepare_ref_with_limits(&chain, limits),
        Err(PrepareError::RelatedDepthExceeded { depth: 1, limit: 0 })
    );

    let limits = PreparationLimits {
        max_item_text_bytes: 3,
        ..PreparationLimits::default()
    };
    assert_eq!(
        prepare_ref_with_limits(&Chain(None), limits),
        Err(PrepareError::ItemTooLarge {
            item: PreparationItemKind::Message,
            bytes: 5,
            limit: 3,
        })
    );

    let limits = PreparationLimits {
        max_total_items: 0,
        ..PreparationLimits::default()
    };
    assert_eq!(
        prepare_ref_with_limits(&Chain(None), limits),
        Err(PrepareError::ItemLimitExceeded {
            attempted: 1,
            limit: 0,
        })
    );

    let limits = PreparationLimits {
        max_total_text_bytes: 4,
        ..PreparationLimits::default()
    };
    assert_eq!(
        prepare_ref_with_limits(&Chain(None), limits),
        Err(PrepareError::TextLimitExceeded {
            attempted: 5,
            limit: 4,
        })
    );
}

#[derive(Debug)]
struct TransparentInner;

impl Diagnostic for TransparentInner {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("inner")
    }
}

#[derive(Debug)]
#[repr(transparent)]
struct TransparentOuter(TransparentInner);

impl Diagnostic for TransparentOuter {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("outer")
    }

    fn visit(&self, visitor: &mut dyn VisitDiagnostic) {
        visitor.related(&self.0);
    }
}

#[test]
fn ambiguous_same_address_nodes_are_rejected_conservatively() {
    let outer = TransparentOuter(TransparentInner);
    assert_eq!(
        (&outer as *const TransparentOuter).cast::<()>(),
        (&outer.0 as *const TransparentInner).cast::<()>(),
    );
    assert_eq!(
        prepare_ref(&outer),
        Err(PrepareError::DiagnosticCycle {
            relation: DiagnosticRelation::Related,
            depth: 1,
        })
    );
}

#[derive(Debug)]
struct CyclicCause;

impl fmt::Display for CyclicCause {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("cyclic cause")
    }
}

impl Error for CyclicCause {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self)
    }
}

static CYCLIC_CAUSE: CyclicCause = CyclicCause;

#[derive(Debug)]
struct CauseDiagnostic;

impl Diagnostic for CauseDiagnostic {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("cause root")
    }

    fn cause(&self) -> Option<&(dyn Error + 'static)> {
        Some(&CYCLIC_CAUSE)
    }
}

#[test]
fn conventional_causes_have_independent_cycle_and_depth_bounds() {
    assert_eq!(prepare_ref(&CauseDiagnostic), Err(PrepareError::CauseCycle { depth: 2 }));
    let limits = PreparationLimits {
        max_cause_depth: 0,
        ..PreparationLimits::default()
    };
    assert_eq!(
        prepare_ref_with_limits(&CauseDiagnostic, limits),
        Err(PrepareError::CauseDepthExceeded { depth: 1, limit: 0 })
    );
}

#[derive(Debug)]
struct MutableMetadata {
    error: Arc<AtomicBool>,
}

impl Diagnostic for MutableMetadata {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("mutable")
    }

    fn descriptor(&self) -> Option<&'static DiagnosticDescriptor> {
        self.error.load(Ordering::SeqCst).then_some(&DESCRIPTOR)
    }

    fn severity(&self) -> Severity {
        if self.error.load(Ordering::SeqCst) {
            Severity::Error
        } else {
            Severity::Warning
        }
    }

    fn tags(&self) -> &[DiagnosticTag] {
        if self.error.load(Ordering::SeqCst) {
            TAGS
        } else {
            &[]
        }
    }
}

#[test]
fn set_preparation_uses_stored_metadata_and_stable_ids_repeatably() {
    let state = Arc::new(AtomicBool::new(false));
    let mut collector = DiagnosticCollector::new();
    collector.add_owned(
        OwnedDiagnostic::new(MutableMetadata {
            error: Arc::clone(&state),
        })
        .with_context("outer context"),
    );
    state.store(true, Ordering::SeqCst);
    let set = collector.finish();
    let sources = SourceMap::new(SourceNamespace(10));

    let first = set.prepare(&sources).unwrap();
    let second = set.prepare(&sources).unwrap();
    let first = &first.iter().next().unwrap().snapshot;
    let second = &second.iter().next().unwrap().snapshot;

    assert_eq!(first.instance_id.unwrap().get(), 0);
    assert_eq!(first.instance_id, second.instance_id);
    assert_eq!(first.severity, Severity::Warning);
    assert_eq!(first.descriptor, None);
    assert_eq!(first.code, None);
    assert!(first.tags.is_empty());
    assert_eq!(first.contexts, ["outer context"]);
}

#[test]
fn prepared_contexts_are_presented_outermost_first_without_reordering_storage() {
    let report = Err::<(), _>(Unlocated("failed"))
        .wrap_err("inner context")
        .wrap_err("outer context")
        .unwrap_err();
    assert_eq!(
        report.contexts().iter().map(|context| context.message()).collect::<Vec<_>>(),
        ["inner context", "outer context"]
    );

    let report_debug = std::format!("{report:?}");
    let outer = report_debug.find("context: outer context").unwrap();
    let inner = report_debug.find("context: inner context").unwrap();
    assert!(outer < inner, "outer context must precede inner context:\n{report_debug}");

    let mut collector = DiagnosticCollector::new();
    collector.add_owned(report.into_diagnostic());
    let set = collector.finish();
    let sources = SourceMap::new(SourceNamespace(11));
    let prepared = set.prepare(&sources).unwrap();
    let diagnostic = prepared.iter().next().unwrap();

    assert_eq!(diagnostic.snapshot.contexts, ["outer context", "inner context"]);

    let rendered = AnnotateRenderer::default().render(diagnostic).unwrap();
    let outer = rendered.find("context: outer context").unwrap();
    let inner = rendered.find("context: inner context").unwrap();
    assert!(outer < inner, "outer context must render before inner context:\n{rendered}");
}

#[test]
fn failed_set_preparation_is_atomic_and_leaves_the_set_reusable() {
    let mut collector = DiagnosticCollector::new();
    collector.add(Unlocated("good"));
    collector.add(FormattingFailure);
    let set = collector.finish();
    let sources = SourceMap::new(SourceNamespace(11));
    assert!(matches!(set.prepare(&sources), Err(PrepareError::MessageFormatting)));
    assert!(matches!(set.prepare(&sources), Err(PrepareError::MessageFormatting)));
    assert_eq!(set.len(), 2);
    assert_eq!(set.iter().map(|entry| entry.id.get()).collect::<Vec<_>>(), [0, 1]);
}

#[derive(Debug)]
struct CauseError;

impl fmt::Display for CauseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("conventional cause")
    }
}

impl Error for CauseError {}

#[derive(Debug)]
struct ChildDiagnostic(&'static str);

impl Diagnostic for ChildDiagnostic {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str(self.0)
    }

    fn severity(&self) -> Severity {
        Severity::Info
    }
}

#[derive(Debug)]
struct RichDiagnostic {
    session: SourceSpan,
    attached: SourceSpan,
    source: ChildDiagnostic,
    related: ChildDiagnostic,
    cause: CauseError,
}

impl Diagnostic for RichDiagnostic {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("unexpected token")
    }

    fn descriptor(&self) -> Option<&'static DiagnosticDescriptor> {
        Some(&DESCRIPTOR)
    }

    fn visit(&self, visitor: &mut dyn VisitDiagnostic) {
        visitor.label(Label {
            span: self.session,
            style: LabelStyle::Primary,
            message: Some(format_args!("unexpected")),
        });
        visitor.label(Label {
            span: self.attached,
            style: LabelStyle::Context,
            message: Some(format_args!("declared here")),
        });
        visitor.note(Note {
            kind: NoteKind::Note,
            message: format_args!("parser note"),
        });
        visitor.note(Note {
            kind: NoteKind::Help,
            message: format_args!("parser help"),
        });
        let edits = [
            TextEdit {
                span: self.session,
                replacement: format_args!("proc"),
            },
            TextEdit {
                span: self.attached,
                replacement: format_args!("let"),
            },
        ];
        visitor.suggestion(Suggestion {
            message: format_args!("apply both edits"),
            applicability: Applicability::MachineApplicable,
            edits: &edits,
        });
        visitor.related(&self.related);
    }

    fn cause(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.cause)
    }

    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        Some(&self.source)
    }
}

fn rich_prepared() -> (SourceMap, miden_diagnostics::DiagnosticSet, SourceId, SourceId) {
    let mut session = SourceMap::new(SourceNamespace(20));
    let session_id =
        session.insert("same.masm", "fn main() {}\n", Some(SourceRevision(7))).unwrap();
    let mut attached = SourceMap::new(SourceNamespace(20));
    let attached_id = attached.insert("same.masm", "var value = 1\n", None).unwrap();
    let diagnostic = RichDiagnostic {
        session: SourceSpan::session(session_id, range(0, 2)).with_revision(SourceRevision(7)),
        attached: SourceSpan::attached(attached_id, range(0, 3)),
        source: ChildDiagnostic("inner source"),
        related: ChildDiagnostic("related message"),
        cause: CauseError,
    };
    let mut collector = DiagnosticCollector::new();
    collector.add_owned(
        OwnedDiagnostic::new(diagnostic)
            .with_context("while parsing module")
            .attach_sources(attached),
    );
    (session, collector.finish(), session_id, attached_id)
}

#[test]
fn renderer_lowers_full_semantics_and_never_reads_explanations() {
    let (session, set, ..) = rich_prepared();
    let prepared = set.prepare(&session).unwrap();
    let diagnostic = prepared.iter().next().unwrap();

    let plain = AnnotateRenderer::default().render(diagnostic).unwrap();
    assert!(plain.contains("error[miden::parser/E0007]: unexpected token"));
    assert!(plain.contains("same.masm [session 20:0]"));
    assert!(plain.contains("same.masm [attached 20:0]"));
    assert!(plain.contains("while parsing module"));
    assert!(plain.contains("conventional cause"));
    assert!(plain.contains("inner source"));
    assert!(plain.contains("apply both edits"));
    assert!(plain.contains("related message"));
    assert!(!plain.contains("EXPLANATION_SENTINEL_MUST_NOT_RENDER"));
    assert!(plain.chars().all(|character| { character == '\n' || !character.is_control() }));

    let short = AnnotateRenderer::new(RenderConfig {
        short: true,
        ..RenderConfig::default()
    })
    .render(diagnostic)
    .unwrap();
    assert!(short.contains("unexpected token"));
    assert!(!short.contains("apply both edits"));
    assert!(!short.contains("related message"));
}

#[test]
fn renderer_output_matrix_matches_checked_in_goldens() {
    let (session, set, ..) = rich_prepared();
    let prepared = set.prepare(&session).unwrap();
    let diagnostic = prepared.iter().next().unwrap();
    let cases = [
        (
            "plain-ascii",
            RenderConfig::default(),
            include_str!("goldens/render_plain_ascii.txt"),
        ),
        (
            "plain-unicode",
            RenderConfig {
                unicode: true,
                ..RenderConfig::default()
            },
            include_str!("goldens/render_plain_unicode.txt"),
        ),
        (
            "styled",
            RenderConfig {
                styled: true,
                ..RenderConfig::default()
            },
            include_str!("goldens/render_styled.txt"),
        ),
        (
            "short",
            RenderConfig {
                short: true,
                ..RenderConfig::default()
            },
            include_str!("goldens/render_short.txt"),
        ),
        (
            "anonymized",
            RenderConfig {
                anonymize_line_numbers: true,
                width: 72,
                ..RenderConfig::default()
            },
            include_str!("goldens/render_anonymized.txt"),
        ),
    ];
    let mut mismatches = String::new();
    for (name, config, expected) in cases {
        let actual = visible_controls(&AnnotateRenderer::new(config).render(diagnostic).unwrap());
        if actual.trim_end() != expected.trim_end() {
            writeln!(&mut mismatches, "\n--- {name} ---\n{actual}").unwrap();
        }
    }
    assert!(mismatches.is_empty(), "{mismatches}");
}

fn visible_controls(text: &str) -> String {
    let mut visible = String::new();
    for character in text.chars() {
        match character {
            '\u{1b}' => visible.push_str("<ESC>"),
            '\r' => visible.push_str("<CR>"),
            character => visible.push(character),
        }
    }
    visible
}

fn snapshot_with_label(span: SourceSpan) -> DiagnosticSnapshot {
    DiagnosticSnapshot {
        instance_id: None,
        code: Some(DiagnosticCodeOwned::new("test", "E1")),
        descriptor: None,
        tags: vec![],
        severity: Severity::Error,
        message: "bad input".into(),
        labels: vec![OwnedLabel {
            span,
            style: LabelStyle::Primary,
            message: Some("here".into()),
        }],
        notes: vec![],
        suggestions: vec![],
        causes: vec![],
        diagnostic_source: None,
        related: vec![],
        contexts: vec![],
    }
}

#[test]
fn renderer_rejects_source_revision_range_edit_url_and_width_failures() {
    let missing_id = SourceId::new(SourceNamespace(30), 0);
    let empty = SourceMap::new(SourceNamespace(30));
    let prepared = PreparedDiagnostic {
        snapshot: snapshot_with_label(SourceSpan::session(missing_id, range(0, 0))),
        sources: LayeredSourceProvider::session_only(&empty),
    };
    assert_eq!(
        AnnotateRenderer::default().render(&prepared),
        Err(RenderError::MissingSource(SourceKey::Session(missing_id)))
    );

    let mut sources = SourceMap::new(SourceNamespace(31));
    let id = sources.insert("unicode.masm", "é", Some(SourceRevision(2))).unwrap();
    let prepared = PreparedDiagnostic {
        snapshot: snapshot_with_label(
            SourceSpan::session(id, range(0, 2)).with_revision(SourceRevision(1)),
        ),
        sources: LayeredSourceProvider::session_only(&sources),
    };
    assert_eq!(
        AnnotateRenderer::default().render(&prepared),
        Err(RenderError::StaleRevision {
            source: SourceKey::Session(id),
            requested: SourceRevision(1),
            current: Some(SourceRevision(2)),
        })
    );

    let prepared = PreparedDiagnostic {
        snapshot: snapshot_with_label(SourceSpan::session(id, range(1, 2))),
        sources: LayeredSourceProvider::session_only(&sources),
    };
    assert_eq!(
        AnnotateRenderer::default().render(&prepared),
        Err(RenderError::InvalidUtf8Boundary {
            source: SourceKey::Session(id),
            offset: 1,
        })
    );

    let prepared = PreparedDiagnostic {
        snapshot: snapshot_with_label(SourceSpan::session(id, range(2, 2))),
        sources: LayeredSourceProvider::session_only(&sources),
    };
    assert!(
        AnnotateRenderer::default().render(&prepared).is_ok(),
        "revision-less EOF spans are valid"
    );

    let prepared = PreparedDiagnostic {
        snapshot: snapshot_with_label(SourceSpan::session(id, range(0, 3))),
        sources: LayeredSourceProvider::session_only(&sources),
    };
    assert_eq!(
        AnnotateRenderer::default().render(&prepared),
        Err(RenderError::OutOfBounds {
            source: SourceKey::Session(id),
            range: range(0, 3),
            byte_len: 2,
        })
    );

    let mut snapshot = snapshot_with_label(SourceSpan::session(id, range(0, 2)));
    snapshot.suggestions.push(OwnedSuggestion {
        message: "overlap".into(),
        applicability: Applicability::Unspecified,
        edits: vec![
            OwnedTextEdit {
                span: SourceSpan::session(id, range(0, 2)),
                replacement: "a".into(),
            },
            OwnedTextEdit {
                span: SourceSpan::session(id, range(0, 2)),
                replacement: "b".into(),
            },
        ],
    });
    let prepared = PreparedDiagnostic {
        snapshot,
        sources: LayeredSourceProvider::session_only(&sources),
    };
    assert!(matches!(
        AnnotateRenderer::default().render(&prepared),
        Err(RenderError::OverlappingEdits { .. })
    ));

    let mut snapshot = snapshot_with_label(SourceSpan::session(id, range(0, 2)));
    snapshot.suggestions.push(OwnedSuggestion {
        message: "empty".into(),
        applicability: Applicability::Unspecified,
        edits: vec![],
    });
    let prepared = PreparedDiagnostic {
        snapshot,
        sources: LayeredSourceProvider::session_only(&sources),
    };
    assert_eq!(
        AnnotateRenderer::default().render(&prepared),
        Err(RenderError::EmptySuggestion { index: 0 })
    );

    let mut snapshot = snapshot_with_label(SourceSpan::session(id, range(0, 2)));
    snapshot.suggestions.push(OwnedSuggestion {
        message: "ambiguous".into(),
        applicability: Applicability::Unspecified,
        edits: vec![
            OwnedTextEdit {
                span: SourceSpan::session(id, range(2, 2)),
                replacement: "a".into(),
            },
            OwnedTextEdit {
                span: SourceSpan::session(id, range(2, 2)),
                replacement: "b".into(),
            },
        ],
    });
    let prepared = PreparedDiagnostic {
        snapshot,
        sources: LayeredSourceProvider::session_only(&sources),
    };
    assert_eq!(
        AnnotateRenderer::default().render(&prepared),
        Err(RenderError::AmbiguousInsertions {
            source: SourceKey::Session(id),
            offset: 2,
        })
    );

    let mut snapshot = snapshot_with_label(SourceSpan::session(id, range(0, 2)));
    snapshot.labels.push(OwnedLabel {
        span: SourceSpan::session(id, range(0, 2)),
        style: LabelStyle::Primary,
        message: None,
    });
    let prepared = PreparedDiagnostic {
        snapshot,
        sources: LayeredSourceProvider::session_only(&sources),
    };
    assert_eq!(
        AnnotateRenderer::default().render(&prepared),
        Err(RenderError::MultiplePrimaryLabels { count: 2 })
    );

    let mut snapshot = snapshot_with_label(SourceSpan::session(id, range(0, 2)));
    snapshot.descriptor = Some(&INVALID_URL_DESCRIPTOR);
    let prepared = PreparedDiagnostic {
        snapshot,
        sources: LayeredSourceProvider::session_only(&sources),
    };
    assert_eq!(
        AnnotateRenderer::new(RenderConfig {
            hyperlinks: true,
            ..RenderConfig::default()
        })
        .render(&prepared),
        Err(RenderError::InvalidDocumentationUrl)
    );
    assert!(
        AnnotateRenderer::new(RenderConfig::default()).render(&prepared).is_ok(),
        "a dormant URL is never passed to the backend"
    );

    assert_eq!(
        AnnotateRenderer::new(RenderConfig {
            width: 0,
            ..RenderConfig::default()
        })
        .render(&prepared),
        Err(RenderError::InvalidWidth { width: 0 })
    );
}

struct CorruptProvider {
    requested: SourceId,
    returned: SourceId,
    declared: u32,
    text: &'static str,
}

impl SourceProvider for CorruptProvider {
    fn get(&self, id: SourceId) -> Option<Source<'_>> {
        (id == self.requested).then_some(Source {
            id: self.returned,
            display_name: "corrupt",
            byte_len: self.declared,
            text: Some(self.text),
            revision: None,
        })
    }

    fn line_column(&self, _id: SourceId, _offset: u32) -> Option<LineColumn> {
        None
    }
}

#[test]
fn renderer_distinguishes_provider_corruption() {
    let id = SourceId::new(SourceNamespace(40), 0);
    let other = SourceId::new(SourceNamespace(40), 1);
    let provider = CorruptProvider {
        requested: id,
        returned: other,
        declared: 1,
        text: "x",
    };
    let prepared = PreparedDiagnostic {
        snapshot: snapshot_with_label(SourceSpan::session(id, range(0, 1))),
        sources: LayeredSourceProvider::session_only(&provider),
    };
    assert_eq!(
        AnnotateRenderer::default().render(&prepared),
        Err(RenderError::ProviderIdMismatch {
            source: SourceKey::Session(id),
            returned: other,
        })
    );

    let provider = CorruptProvider {
        requested: id,
        returned: id,
        declared: 2,
        text: "x",
    };
    let prepared = PreparedDiagnostic {
        snapshot: snapshot_with_label(SourceSpan::session(id, range(0, 1))),
        sources: LayeredSourceProvider::session_only(&provider),
    };
    assert_eq!(
        AnnotateRenderer::default().render(&prepared),
        Err(RenderError::ProviderByteLengthMismatch {
            source: SourceKey::Session(id),
            declared: 2,
            actual: 1,
        })
    );
}

struct MetadataProvider {
    id: SourceId,
    locations: Vec<(u32, LineColumn)>,
}

impl SourceProvider for MetadataProvider {
    fn get(&self, id: SourceId) -> Option<Source<'_>> {
        (id == self.id).then_some(Source {
            id,
            display_name: "generated.masm",
            byte_len: 8,
            text: None,
            revision: None,
        })
    }

    fn line_column(&self, id: SourceId, offset: u32) -> Option<LineColumn> {
        (id == self.id)
            .then(|| {
                self.locations
                    .iter()
                    .find_map(|(candidate, location)| (*candidate == offset).then_some(*location))
            })
            .flatten()
    }
}

#[test]
fn metadata_only_sources_preserve_each_label_role_and_location() {
    let id = SourceId::new(SourceNamespace(41), 0);
    let provider = MetadataProvider {
        id,
        locations: vec![
            (0, LineColumn::new(12, 4).unwrap()),
            (2, LineColumn::new(12, 6).unwrap()),
            (4, LineColumn::new(20, 1).unwrap()),
            (6, LineColumn::new(20, 3).unwrap()),
        ],
    };
    let mut snapshot = snapshot_with_label(SourceSpan::session(id, range(0, 2)));
    snapshot.labels.push(OwnedLabel {
        span: SourceSpan::session(id, range(4, 6)),
        style: LabelStyle::Context,
        message: Some("secondary metadata label".into()),
    });
    let prepared = PreparedDiagnostic {
        snapshot,
        sources: LayeredSourceProvider::session_only(&provider),
    };
    let output = AnnotateRenderer::default().render(&prepared).unwrap();
    assert!(output.contains("generated.masm:12:4"));
    assert!(output.contains("primary at generated.masm:12:4: here"));
    assert!(output.contains("context at generated.masm:20:1: secondary metadata label"));

    let mut snapshot = snapshot_with_label(SourceSpan::session(id, range(0, 2)));
    snapshot.suggestions.push(OwnedSuggestion {
        message: "needs text".into(),
        applicability: Applicability::Unspecified,
        edits: vec![OwnedTextEdit {
            span: SourceSpan::session(id, range(0, 2)),
            replacement: "x".into(),
        }],
    });
    let prepared = PreparedDiagnostic {
        snapshot,
        sources: LayeredSourceProvider::session_only(&provider),
    };
    assert_eq!(
        AnnotateRenderer::default().render(&prepared),
        Err(RenderError::MissingSourceText(SourceKey::Session(id)))
    );

    let provider = MetadataProvider {
        id,
        locations: vec![(0, LineColumn::new(12, 4).unwrap())],
    };
    let prepared = PreparedDiagnostic {
        snapshot: snapshot_with_label(SourceSpan::session(id, range(0, 2))),
        sources: LayeredSourceProvider::session_only(&provider),
    };
    assert_eq!(
        AnnotateRenderer::default().render(&prepared),
        Err(RenderError::MetadataLocationUnavailable {
            source: SourceKey::Session(id),
            offset: 2,
        })
    );
}

#[test]
fn renderer_enforces_terminal_safety_and_explicit_hyperlinks() {
    let sources = SourceMap::new(SourceNamespace(50));
    let mut snapshot = snapshot_with_label(SourceSpan::session(
        SourceId::new(SourceNamespace(50), 0),
        range(0, 0),
    ));
    snapshot.labels.clear();
    snapshot.message = "bad\u{1b}[31m\u{009b}message".into();
    snapshot.code = Some(DiagnosticCodeOwned::new("na\u{1b}mespace", "E\u{009d}1"));
    snapshot.notes = vec![OwnedNote {
        kind: NoteKind::Help,
        message: "help\u{7}\u{009c}".into(),
    }];
    snapshot.descriptor = Some(&DESCRIPTOR);
    let prepared = PreparedDiagnostic {
        snapshot,
        sources: LayeredSourceProvider::session_only(&sources),
    };

    let plain = AnnotateRenderer::default().render(&prepared).unwrap();
    assert!(!plain.contains('\u{1b}'));
    assert!(!plain.contains('\u{009b}'));
    assert!(!plain.contains("https://docs.example.test/E0007"));
    assert!(plain.chars().all(|character| { character == '\n' || !character.is_control() }));

    let linked = AnnotateRenderer::new(RenderConfig {
        hyperlinks: true,
        ..RenderConfig::default()
    })
    .render(&prepared)
    .unwrap();
    assert!(linked.contains("https://docs.example.test/E0007"));
    assert!(linked.contains("\u{1b}]8;;https://docs.example.test/E0007\u{1b}\\"));
    assert!(!linked.contains('\u{009b}'));

    let styled = AnnotateRenderer::new(RenderConfig {
        styled: true,
        ..RenderConfig::default()
    })
    .render(&prepared)
    .unwrap();
    assert!(styled.contains("\u{1b}["));
    assert!(!styled.contains("\u{1b}]"));
    assert!(!styled.contains('\u{009b}'));
}

#[test]
fn every_rendered_user_field_crosses_the_terminal_safety_boundary() {
    let mut sources = SourceMap::new(SourceNamespace(51));
    let id = sources
        .insert(
            "pa\u{1b}]0;title\u{7}th\u{009d}.masm",
            concat!(
                "\u{1b}[2J",
                "\u{1b}]8;;https://docs.example.test/E0007\u{1b}\\",
                "injected",
                "\u{1b}]8;;\u{1b}\\",
                "x\u{009b}31m\n",
            ),
            None,
        )
        .unwrap();
    let span = SourceSpan::session(id, range(0, 1));
    let mut snapshot = snapshot_with_label(span);
    snapshot.message = "title\u{0}\u{1b}\u{009b}".into();
    snapshot.code = Some(DiagnosticCodeOwned::new("ns\u{7}", "E\u{009d}"));
    snapshot.descriptor = Some(&DESCRIPTOR);
    snapshot.labels[0].message = Some("label\u{1b}\u{009c}".into());
    snapshot.contexts = vec!["context\u{8}\u{009b}".into()];
    snapshot.notes = vec![
        OwnedNote {
            kind: NoteKind::Note,
            message: "note\u{1b}\u{009d}".into(),
        },
        OwnedNote {
            kind: NoteKind::Help,
            message: "help\u{7}\u{009c}".into(),
        },
    ];
    snapshot.causes = vec![OwnedCause {
        message: "cause\u{1b}\u{009b}".into(),
    }];
    snapshot.suggestions = vec![OwnedSuggestion {
        message: "suggestion\u{1b}\u{009d}".into(),
        applicability: Applicability::MaybeIncorrect,
        edits: vec![OwnedTextEdit {
            span,
            replacement: "replacement\u{1b}\u{009c}".into(),
        }],
    }];
    snapshot.related = vec![DiagnosticSnapshot {
        instance_id: None,
        code: None,
        descriptor: None,
        tags: vec![],
        severity: Severity::Info,
        message: "related\u{1b}\u{009b}".into(),
        labels: vec![],
        notes: vec![],
        suggestions: vec![],
        causes: vec![],
        diagnostic_source: None,
        related: vec![],
        contexts: vec![],
    }];
    let prepared = PreparedDiagnostic {
        snapshot,
        sources: LayeredSourceProvider::session_only(&sources),
    };

    let plain = AnnotateRenderer::default().render(&prepared).unwrap();
    assert!(
        plain.chars().all(|character| character == '\n' || !character.is_control()),
        "{plain:?}"
    );
    let styled_and_linked = AnnotateRenderer::new(RenderConfig {
        styled: true,
        hyperlinks: true,
        ..RenderConfig::default()
    })
    .render(&prepared)
    .unwrap();
    assert!(
        !styled_and_linked
            .chars()
            .any(|character| { ('\u{80}'..='\u{9f}').contains(&character) })
    );
    assert!(!styled_and_linked.contains("\u{1b}]0;"));
    assert!(!styled_and_linked.contains("\u{1b}[2J"));
    assert!(styled_and_linked.contains("\u{1b}]8;;https://docs.example.test/E0007\u{1b}\\"));
    assert_eq!(
        styled_and_linked
            .matches("\u{1b}]8;;https://docs.example.test/E0007\u{1b}\\")
            .count(),
        1,
        "only the renderer-generated title hyperlink may survive"
    );
    assert_eq!(
        styled_and_linked.matches("\u{1b}]8;;\u{1b}\\").count(),
        1,
        "source-injected OSC-8 closes must be normalized with their opens"
    );
}

#[derive(Debug)]
struct Unlocated(&'static str);

impl Diagnostic for Unlocated {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str(self.0)
    }
}

#[derive(Debug)]
struct MissingLocated(SourceSpan);

impl Diagnostic for MissingLocated {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("missing location")
    }

    fn visit(&self, visitor: &mut dyn VisitDiagnostic) {
        visitor.label(Label {
            span: self.0,
            style: LabelStyle::Primary,
            message: Some(format_args!("missing")),
        });
    }
}

#[test]
#[cfg(feature = "std")]
fn fmt_and_io_emitters_are_repeatable_and_report_degradation() {
    let namespace = SourceNamespace(60);
    let missing = SourceSpan::session(SourceId::new(namespace, 0), range(0, 0));
    let mut collector = DiagnosticCollector::new();
    collector.add(Unlocated("first"));
    collector.add(MissingLocated(missing));
    let set = collector.finish();
    let sources = SourceMap::new(namespace);
    let prepared = set.prepare(&sources).unwrap();

    let mut output = String::new();
    let mut emitter = FmtEmitter::new(&mut output, AnnotateRenderer::default());
    let summary = emitter.emit_set(&prepared).unwrap();
    assert_eq!(summary.emitted, 2);
    assert_eq!(summary.degraded, 1);
    assert!(output.contains("error: first"));
    assert!(output.contains("diagnostic rendering degraded: missing source"));
    assert_eq!(output.lines().filter(|line| *line == "error: first").count(), 1);

    let mut second_output = String::new();
    let mut second = FmtEmitter::new(&mut second_output, AnnotateRenderer::default());
    assert_eq!(second.emit_set(&prepared).unwrap(), summary);
    assert_eq!(second_output, output);

    let writer = TrackingWriter::default();
    let mut io = IoEmitter::new(writer, AnnotateRenderer::default());
    assert_eq!(io.emit_set(&prepared).unwrap(), summary);
    let writer = io.into_inner();
    assert_eq!(writer.flushes, 1);
    assert_eq!(String::from_utf8(writer.bytes).unwrap(), output);
}

#[derive(Default)]
#[cfg(feature = "std")]
struct TrackingWriter {
    bytes: Vec<u8>,
    flushes: usize,
    fail_flush: bool,
}

#[cfg(feature = "std")]
impl std::io::Write for TrackingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flushes += 1;
        if self.fail_flush {
            Err(std::io::Error::other("flush failed"))
        } else {
            Ok(())
        }
    }
}

#[test]
#[cfg(feature = "std")]
fn io_emitter_flush_failure_reports_the_completed_prefix() {
    let mut collector = DiagnosticCollector::new();
    collector.add(Unlocated("one"));
    collector.add(Unlocated("two"));
    let set = collector.finish();
    let sources = SourceMap::new(SourceNamespace(61));
    let prepared = set.prepare(&sources).unwrap();
    let writer = TrackingWriter {
        fail_flush: true,
        ..TrackingWriter::default()
    };
    let mut emitter = IoEmitter::new(writer, AnnotateRenderer::default());
    let failure = emitter.emit_set(&prepared).unwrap_err();
    assert!(matches!(failure.error, IoEmissionError::Flush(_)));
    assert_eq!(failure.completed.emitted, 2);
    assert_eq!(failure.completed.degraded, 0);
}

#[cfg(feature = "std")]
struct FailingIoWriter {
    bytes: Vec<u8>,
    writes: usize,
    fail_on_write: usize,
    flushes: usize,
}

#[cfg(feature = "std")]
impl std::io::Write for FailingIoWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.writes == self.fail_on_write {
            return Err(std::io::Error::other("write failed"));
        }
        self.writes += 1;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}

#[test]
#[cfg(feature = "std")]
fn io_write_failure_stops_without_flush_and_reports_only_complete_records() {
    let mut collector = DiagnosticCollector::new();
    collector.add(Unlocated("one"));
    collector.add(Unlocated("two"));
    let set = collector.finish();
    let sources = SourceMap::new(SourceNamespace(62));
    let prepared = set.prepare(&sources).unwrap();
    let writer = FailingIoWriter {
        bytes: vec![],
        writes: 0,
        fail_on_write: 2,
        flushes: 0,
    };
    let mut emitter = IoEmitter::new(writer, AnnotateRenderer::default());
    let failure = emitter.emit_set(&prepared).unwrap_err();
    assert!(matches!(failure.error, IoEmissionError::Write(_)));
    assert_eq!(failure.completed.emitted, 1);
    assert_eq!(emitter.writer().flushes, 0);
}

#[test]
#[cfg(feature = "std")]
fn empty_io_set_flushes_once_without_emitting_a_record() {
    let set = DiagnosticCollector::new().finish();
    let sources = SourceMap::new(SourceNamespace(63));
    let prepared = set.prepare(&sources).unwrap();
    let mut emitter = IoEmitter::new(TrackingWriter::default(), AnnotateRenderer::default());
    let summary = emitter.emit_set(&prepared).unwrap();
    assert_eq!(summary.emitted, 0);
    assert_eq!(summary.degraded, 0);
    let writer = emitter.into_inner();
    assert!(writer.bytes.is_empty());
    assert_eq!(writer.flushes, 1);
}

struct FailingFmtWriter {
    writes: usize,
    fail_on_write: usize,
}

impl fmt::Write for FailingFmtWriter {
    fn write_str(&mut self, _text: &str) -> fmt::Result {
        if self.writes == self.fail_on_write {
            return Err(fmt::Error);
        }
        self.writes += 1;
        Ok(())
    }
}

#[test]
fn sink_failure_while_writing_a_degraded_record_does_not_count_it() {
    let namespace = SourceNamespace(64);
    let mut collector = DiagnosticCollector::new();
    collector.add(MissingLocated(SourceSpan::session(SourceId::new(namespace, 0), range(0, 0))));
    let set = collector.finish();
    let sources = SourceMap::new(namespace);
    let prepared = set.prepare(&sources).unwrap();
    let mut emitter = FmtEmitter::new(
        FailingFmtWriter {
            writes: 0,
            fail_on_write: 0,
        },
        AnnotateRenderer::default(),
    );
    let failure = emitter.emit_set(&prepared).unwrap_err();
    assert_eq!(failure.completed.emitted, 0);
    assert_eq!(failure.completed.degraded, 0);
}

#[test]
fn deterministic_malformed_corpus_never_unwinds_through_the_renderer() {
    let mut sources = SourceMap::new(SourceNamespace(70));
    let id = sources.insert("fuzz.masm", "éx", None).unwrap();
    let mut empty_suggestion = snapshot_with_label(SourceSpan::session(id, range(0, 2)));
    empty_suggestion.suggestions.push(OwnedSuggestion {
        message: "empty".into(),
        applicability: Applicability::Unspecified,
        edits: vec![],
    });
    let mut overlapping = snapshot_with_label(SourceSpan::session(id, range(0, 2)));
    overlapping.suggestions.push(OwnedSuggestion {
        message: "overlap".into(),
        applicability: Applicability::Unspecified,
        edits: vec![
            OwnedTextEdit {
                span: SourceSpan::session(id, range(0, 2)),
                replacement: "a".into(),
            },
            OwnedTextEdit {
                span: SourceSpan::session(id, range(0, 2)),
                replacement: "b".into(),
            },
        ],
    });
    let mut ambiguous = snapshot_with_label(SourceSpan::session(id, range(0, 2)));
    ambiguous.suggestions.push(OwnedSuggestion {
        message: "ambiguous".into(),
        applicability: Applicability::Unspecified,
        edits: vec![
            OwnedTextEdit {
                span: SourceSpan::session(id, range(2, 2)),
                replacement: "a".into(),
            },
            OwnedTextEdit {
                span: SourceSpan::session(id, range(2, 2)),
                replacement: "b".into(),
            },
        ],
    });
    let mut multiple_primary = snapshot_with_label(SourceSpan::session(id, range(0, 2)));
    multiple_primary.labels.push(OwnedLabel {
        span: SourceSpan::session(id, range(0, 2)),
        style: LabelStyle::Primary,
        message: None,
    });
    let cases = vec![
        snapshot_with_label(SourceSpan::session(id, range(0, 4))),
        snapshot_with_label(SourceSpan::session(id, range(1, 2))),
        snapshot_with_label(SourceSpan::session(id, range(0, 2)).with_revision(SourceRevision(99))),
        empty_suggestion,
        overlapping,
        ambiguous,
        multiple_primary,
    ];
    for snapshot in cases {
        let prepared = PreparedDiagnostic {
            snapshot,
            sources: LayeredSourceProvider::session_only(&sources),
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            AnnotateRenderer::default().render(&prepared)
        }));
        assert!(result.is_ok());
        assert!(result.unwrap().is_err());
    }

    let missing_id = SourceId::new(SourceNamespace(70), 99);
    let missing = PreparedDiagnostic {
        snapshot: snapshot_with_label(SourceSpan::session(missing_id, range(0, 0))),
        sources: LayeredSourceProvider::session_only(&sources),
    };
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            AnnotateRenderer::default().render(&missing)
        }))
        .is_ok()
    );

    let corrupt = CorruptProvider {
        requested: id,
        returned: SourceId::new(SourceNamespace(70), 1),
        declared: 3,
        text: "éx",
    };
    let corrupt = PreparedDiagnostic {
        snapshot: snapshot_with_label(SourceSpan::session(id, range(0, 2))),
        sources: LayeredSourceProvider::session_only(&corrupt),
    };
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            AnnotateRenderer::default().render(&corrupt)
        }))
        .is_ok()
    );

    for width in [0, 1, 65_535, 65_536, usize::MAX] {
        let prepared = PreparedDiagnostic {
            snapshot: snapshot_with_label(SourceSpan::session(id, range(0, 2))),
            sources: LayeredSourceProvider::session_only(&sources),
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            AnnotateRenderer::new(RenderConfig {
                width,
                ..RenderConfig::default()
            })
            .render(&prepared)
        }));
        assert!(result.is_ok(), "width {width} unwound");
        if width == 0 || width > u16::MAX as usize {
            assert_eq!(result.unwrap(), Err(RenderError::InvalidWidth { width }));
        }
    }
}

#[test]
fn all_severities_have_distinct_terminal_names() {
    let sources = SourceMap::new(SourceNamespace(80));
    for (severity, expected) in [
        (Severity::Error, "error: message"),
        (Severity::Warning, "warning: message"),
        (Severity::Info, "info: message"),
        (Severity::Hint, "hint: message"),
    ] {
        let prepared = PreparedDiagnostic {
            snapshot: DiagnosticSnapshot {
                instance_id: None,
                code: None,
                descriptor: None,
                tags: vec![],
                severity,
                message: "message".into(),
                labels: vec![],
                notes: vec![],
                suggestions: vec![],
                causes: vec![OwnedCause {
                    message: "cause".into(),
                }],
                diagnostic_source: None,
                related: vec![],
                contexts: vec![],
            },
            sources: LayeredSourceProvider::session_only(&sources),
        };
        let output = AnnotateRenderer::default().render(&prepared).unwrap();
        assert!(output.contains(expected), "{output}");
    }
}

#[test]
#[cfg(feature = "std")]
fn single_emit_reports_status_and_flushes() {
    let mut collector = DiagnosticCollector::new();
    collector.add(Unlocated("one"));
    let set = collector.finish();
    let sources = SourceMap::new(SourceNamespace(90));
    let prepared = set.prepare(&sources).unwrap();
    let diagnostic = prepared.iter().next().unwrap();

    let writer = TrackingWriter::default();
    let mut emitter = IoEmitter::new(writer, AnnotateRenderer::default());
    assert_eq!(emitter.emit(diagnostic).unwrap(), EmissionStatus::Rendered);
    assert_eq!(emitter.into_inner().flushes, 1);
}
