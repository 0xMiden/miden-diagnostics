use alloc::{boxed::Box, string::String, vec::Vec};
use core::{error::Error, fmt, slice};

use crate::{
    Applicability, Diagnostic, DiagnosticCodeRef, DiagnosticDescriptor, DiagnosticInstanceId,
    DiagnosticMetadata, DiagnosticSet, DiagnosticTag, Label, LabelStyle, LayeredSourceProvider,
    Note, NoteKind, OwnedDiagnostic, SourceProvider, SourceSpan, Suggestion, TextEdit,
    VisitDiagnostic, VisitTextEdit, VisitTextEdits, source::EMPTY_SOURCE_PROVIDER,
};

/// An owned, exact diagnostic code captured for one occurrence.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DiagnosticCodeOwned {
    pub namespace: String,
    pub code: String,
}

impl DiagnosticCodeOwned {
    pub fn new(namespace: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            code: code.into(),
        }
    }

    pub fn as_ref(&self) -> DiagnosticCodeRef<'_> {
        DiagnosticCodeRef {
            namespace: &self.namespace,
            code: &self.code,
        }
    }
}

impl From<DiagnosticCodeRef<'_>> for DiagnosticCodeOwned {
    fn from(code: DiagnosticCodeRef<'_>) -> Self {
        Self::new(code.namespace, code.code)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedLabel {
    pub span: SourceSpan,
    pub style: LabelStyle,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedNote {
    pub kind: NoteKind,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedTextEdit {
    pub span: SourceSpan,
    pub replacement: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedSuggestion {
    pub message: String,
    pub applicability: Applicability,
    pub edits: Vec<OwnedTextEdit>,
}

impl VisitTextEdits for Vec<OwnedTextEdit> {
    fn visit_text_edits(&self, visitor: &mut dyn VisitTextEdit) {
        for edit in self {
            visitor.text_edit(TextEdit {
                span: edit.span,
                replacement: format_args!("{}", edit.replacement),
            });
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedCause {
    pub message: String,
}

/// A backend-neutral, source-text-free snapshot of one diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticSnapshot {
    /// Present only for roots prepared from a finalized diagnostic set.
    pub instance_id: Option<DiagnosticInstanceId>,
    pub code: Option<DiagnosticCodeOwned>,
    pub descriptor: Option<&'static DiagnosticDescriptor>,
    pub tags: Vec<DiagnosticTag>,
    pub severity: crate::Severity,
    pub message: String,
    pub labels: Vec<OwnedLabel>,
    pub notes: Vec<OwnedNote>,
    pub suggestions: Vec<OwnedSuggestion>,
    pub causes: Vec<OwnedCause>,
    pub diagnostic_source: Option<Box<DiagnosticSnapshot>>,
    pub related: Vec<DiagnosticSnapshot>,
    /// Propagation contexts ordered from the outermost frame to the innermost.
    pub contexts: Vec<String>,
}

/// Finite resource bounds applied independently to each prepared root.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparationLimits {
    pub max_related_depth: usize,
    pub max_diagnostic_source_depth: usize,
    pub max_cause_depth: usize,
    pub max_total_items: usize,
    pub max_item_text_bytes: usize,
    pub max_total_text_bytes: usize,
}

impl PreparationLimits {
    pub const DEFAULT: Self = Self {
        max_related_depth: 32,
        max_diagnostic_source_depth: 32,
        max_cause_depth: 64,
        max_total_items: 4096,
        max_item_text_bytes: 64 * 1024,
        max_total_text_bytes: 1024 * 1024,
    };
}

impl Default for PreparationLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreparationItemKind {
    CodeNamespace,
    Code,
    Message,
    LabelMessage,
    NoteMessage,
    SuggestionMessage,
    Replacement,
    Cause,
    Context,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticRelation {
    Related,
    DiagnosticSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrepareError {
    MessageFormatting,
    VisitorFormatting {
        item: PreparationItemKind,
    },
    CauseFormatting {
        depth: usize,
    },
    MultiplePrimaryLabels {
        count: usize,
    },
    EmptySuggestion {
        index: usize,
    },
    ItemTooLarge {
        item: PreparationItemKind,
        bytes: usize,
        limit: usize,
    },
    ItemLimitExceeded {
        attempted: usize,
        limit: usize,
    },
    TextLimitExceeded {
        attempted: usize,
        limit: usize,
    },
    DiagnosticCycle {
        relation: DiagnosticRelation,
        depth: usize,
    },
    RelatedDepthExceeded {
        depth: usize,
        limit: usize,
    },
    DiagnosticSourceDepthExceeded {
        depth: usize,
        limit: usize,
    },
    CauseCycle {
        depth: usize,
    },
    CauseDepthExceeded {
        depth: usize,
        limit: usize,
    },
}

impl fmt::Display for PrepareError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MessageFormatting => formatter.write_str("diagnostic message formatting failed"),
            Self::VisitorFormatting { item } => {
                write!(formatter, "diagnostic {item:?} formatting failed")
            }
            Self::CauseFormatting { depth } => {
                write!(formatter, "diagnostic cause formatting failed at depth {depth}")
            }
            Self::MultiplePrimaryLabels { count } => {
                write!(formatter, "diagnostic supplied {count} primary labels")
            }
            Self::EmptySuggestion { index } => {
                write!(formatter, "diagnostic suggestion {index} contains no edits")
            }
            Self::ItemTooLarge { item, bytes, limit } => {
                write!(formatter, "diagnostic {item:?} uses {bytes} bytes, limit is {limit}")
            }
            Self::ItemLimitExceeded { attempted, limit } => {
                write!(formatter, "diagnostic item count {attempted} exceeds limit {limit}")
            }
            Self::TextLimitExceeded { attempted, limit } => {
                write!(formatter, "diagnostic text uses {attempted} bytes, limit is {limit}")
            }
            Self::DiagnosticCycle { relation, depth } => {
                write!(formatter, "diagnostic {relation:?} cycle detected at depth {depth}")
            }
            Self::RelatedDepthExceeded { depth, limit } => {
                write!(formatter, "related depth {depth} exceeds limit {limit}")
            }
            Self::DiagnosticSourceDepthExceeded { depth, limit } => {
                write!(formatter, "diagnostic-source depth {depth} exceeds limit {limit}")
            }
            Self::CauseCycle { depth } => {
                write!(formatter, "conventional cause cycle detected at depth {depth}")
            }
            Self::CauseDepthExceeded { depth, limit } => {
                write!(formatter, "conventional cause depth {depth} exceeds limit {limit}")
            }
        }
    }
}

impl Error for PrepareError {}

/// A prepared snapshot paired with the exact source universes it references.
pub struct PreparedDiagnostic<'a> {
    pub snapshot: DiagnosticSnapshot,
    pub sources: LayeredSourceProvider<'a>,
}

/// A repeatably iterable prepared diagnostic set.
pub struct PreparedSet<'a> {
    diagnostics: Box<[PreparedDiagnostic<'a>]>,
}

impl<'set> PreparedSet<'set> {
    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub fn iter(&self) -> slice::Iter<'_, PreparedDiagnostic<'set>> {
        self.diagnostics.iter()
    }
}

impl fmt::Display for PreparedSet<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::{AnnotateRenderer, Emitter, FmtEmitter};

        let mut formatter = FmtEmitter::new(f, AnnotateRenderer::default());
        match formatter.emit_set(self) {
            Ok(_) => Ok(()),
            Err(err) => Err(err.error),
        }
    }
}

impl<'set, 'borrow> IntoIterator for &'borrow PreparedSet<'set> {
    type IntoIter = slice::Iter<'borrow, PreparedDiagnostic<'set>>;
    type Item = &'borrow PreparedDiagnostic<'set>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub fn prepare_ref(diagnostic: &dyn Diagnostic) -> Result<DiagnosticSnapshot, PrepareError> {
    prepare_ref_with_limits(diagnostic, PreparationLimits::default())
}

pub fn prepare_ref_with_limits(
    diagnostic: &dyn Diagnostic,
    limits: PreparationLimits,
) -> Result<DiagnosticSnapshot, PrepareError> {
    let metadata = live_metadata(diagnostic);
    prepare_root(diagnostic, metadata, None, &[], limits)
}

pub(crate) fn prepare_owned_ref(
    diagnostic: &OwnedDiagnostic,
) -> Result<DiagnosticSnapshot, PrepareError> {
    prepare_owned_ref_with_limits(diagnostic, PreparationLimits::default())
}

pub(crate) fn prepare_owned_ref_with_limits(
    diagnostic: &OwnedDiagnostic,
    limits: PreparationLimits,
) -> Result<DiagnosticSnapshot, PrepareError> {
    prepare_root(
        diagnostic.as_diagnostic(),
        diagnostic.metadata(),
        None,
        diagnostic.contexts(),
        limits,
    )
}

impl DiagnosticSet {
    /// Prepares this set using the session and attached source providers retained by each
    /// diagnostic occurrence.
    pub fn prepare_attached(&self) -> Result<PreparedSet<'_>, PrepareError> {
        self.prepare_attached_with_limits(PreparationLimits::default())
    }

    /// Prepares this set using retained providers and explicit resource limits.
    pub fn prepare_attached_with_limits(
        &self,
        limits: PreparationLimits,
    ) -> Result<PreparedSet<'_>, PrepareError> {
        self.prepare_entries_with_limits(None, limits)
    }

    pub fn prepare<'a>(
        &'a self,
        session_sources: &'a dyn SourceProvider,
    ) -> Result<PreparedSet<'a>, PrepareError> {
        self.prepare_with_limits(session_sources, PreparationLimits::default())
    }

    pub fn prepare_with_limits<'a>(
        &'a self,
        session_sources: &'a dyn SourceProvider,
        limits: PreparationLimits,
    ) -> Result<PreparedSet<'a>, PrepareError> {
        self.prepare_entries_with_limits(Some(session_sources), limits)
    }

    fn prepare_entries_with_limits<'a>(
        &'a self,
        session_sources: Option<&'a dyn SourceProvider>,
        limits: PreparationLimits,
    ) -> Result<PreparedSet<'a>, PrepareError> {
        let mut diagnostics = Vec::with_capacity(self.len());
        for entry in self.iter() {
            let metadata = entry.metadata();
            let snapshot = prepare_root(
                entry.diagnostic.as_diagnostic(),
                metadata,
                Some(entry.id),
                entry.diagnostic.contexts(),
                limits,
            )?;
            let attached = entry
                .diagnostic
                .attached_sources()
                .map(|sources| sources as &dyn SourceProvider);
            let session = session_sources.unwrap_or_else(|| {
                entry
                    .diagnostic
                    .session_sources()
                    .map_or(&EMPTY_SOURCE_PROVIDER as &dyn SourceProvider, |sources| sources)
            });
            diagnostics.push(PreparedDiagnostic {
                snapshot,
                sources: LayeredSourceProvider::new(session, attached),
            });
        }
        Ok(PreparedSet {
            diagnostics: diagnostics.into_boxed_slice(),
        })
    }
}

fn prepare_root(
    diagnostic: &dyn Diagnostic,
    metadata: DiagnosticMetadata<'_>,
    instance_id: Option<DiagnosticInstanceId>,
    contexts: &[crate::ContextFrame],
    limits: PreparationLimits,
) -> Result<DiagnosticSnapshot, PrepareError> {
    let mut state = PreparationState {
        limits,
        items: 0,
        text_bytes: 0,
    };
    let mut snapshot = state.prepare_diagnostic(
        diagnostic,
        SnapshotMetadata {
            instance_id,
            descriptor: metadata.descriptor,
            code: metadata.code,
            tags: metadata.tags,
            severity: metadata.severity,
        },
        0,
        0,
        None,
        &[],
    )?;
    for context in contexts.iter().rev() {
        state.add_item()?;
        snapshot
            .contexts
            .push(state.capture_exact_text(PreparationItemKind::Context, context.message())?);
    }
    Ok(snapshot)
}

#[derive(Clone, Copy)]
struct SnapshotMetadata<'a> {
    instance_id: Option<DiagnosticInstanceId>,
    descriptor: Option<&'static DiagnosticDescriptor>,
    code: Option<DiagnosticCodeRef<'a>>,
    tags: &'a [DiagnosticTag],
    severity: crate::Severity,
}

struct PreparationState {
    limits: PreparationLimits,
    items: usize,
    text_bytes: usize,
}

impl PreparationState {
    fn prepare_diagnostic(
        &mut self,
        diagnostic: &dyn Diagnostic,
        metadata: SnapshotMetadata<'_>,
        related_depth: usize,
        diagnostic_source_depth: usize,
        incoming: Option<DiagnosticRelation>,
        active_diagnostics: &[&dyn Diagnostic],
    ) -> Result<DiagnosticSnapshot, PrepareError> {
        if related_depth > self.limits.max_related_depth {
            return Err(PrepareError::RelatedDepthExceeded {
                depth: related_depth,
                limit: self.limits.max_related_depth,
            });
        }
        if diagnostic_source_depth > self.limits.max_diagnostic_source_depth {
            return Err(PrepareError::DiagnosticSourceDepthExceeded {
                depth: diagnostic_source_depth,
                limit: self.limits.max_diagnostic_source_depth,
            });
        }

        if active_diagnostics.iter().any(|active| core::ptr::eq(*active, diagnostic)) {
            let relation = incoming.unwrap_or(DiagnosticRelation::Related);
            let depth = match relation {
                DiagnosticRelation::Related => related_depth,
                DiagnosticRelation::DiagnosticSource => diagnostic_source_depth,
            };
            return Err(PrepareError::DiagnosticCycle { relation, depth });
        }
        // Keep full trait-object references in this invocation-local stack. Comparing full trait
        // objects includes their vtables, so a diagnostic and an embedded field at the same data
        // address are not mistaken for a cycle.
        let mut active_diagnostics = active_diagnostics.to_vec();
        active_diagnostics.push(diagnostic);
        self.prepare_active_diagnostic(
            diagnostic,
            metadata,
            related_depth,
            diagnostic_source_depth,
            &active_diagnostics,
        )
    }

    fn prepare_active_diagnostic(
        &mut self,
        diagnostic: &dyn Diagnostic,
        metadata: SnapshotMetadata<'_>,
        related_depth: usize,
        diagnostic_source_depth: usize,
        active_diagnostics: &[&dyn Diagnostic],
    ) -> Result<DiagnosticSnapshot, PrepareError> {
        self.add_item()?;
        let code = metadata
            .code
            .map(|code| {
                Ok(DiagnosticCodeOwned {
                    namespace: self
                        .capture_exact_text(PreparationItemKind::CodeNamespace, code.namespace)?,
                    code: self.capture_exact_text(PreparationItemKind::Code, code.code)?,
                })
            })
            .transpose()?;
        let message = self
            .capture_text(PreparationItemKind::Message, |out| diagnostic.message(out))
            .map_err(|error| match error {
                CaptureError::Formatting => PrepareError::MessageFormatting,
                CaptureError::Preparation(error) => error,
            })?;

        let mut visitor = SnapshotVisitor {
            state: self,
            labels: Vec::new(),
            notes: Vec::new(),
            suggestions: Vec::new(),
            related: Vec::new(),
            related_depth,
            diagnostic_source_depth,
            active_diagnostics,
            error: None,
        };
        diagnostic.visit(&mut visitor);
        if let Some(error) = visitor.error {
            return Err(error);
        }
        let SnapshotVisitor {
            state,
            mut labels,
            notes,
            suggestions,
            related,
            ..
        } = visitor;

        let primary_count =
            labels.iter().filter(|label| label.style == LabelStyle::Primary).count();
        if primary_count > 1 {
            return Err(PrepareError::MultiplePrimaryLabels {
                count: primary_count,
            });
        }
        if primary_count == 0
            && let Some(first) = labels.first_mut()
        {
            first.style = LabelStyle::Primary;
        }

        let causes = state.prepare_causes(diagnostic.cause())?;
        let diagnostic_source = diagnostic
            .diagnostic_source()
            .map(|source| {
                let live = live_metadata(source);
                let next_depth = diagnostic_source_depth.checked_add(1).ok_or(
                    PrepareError::DiagnosticSourceDepthExceeded {
                        depth: usize::MAX,
                        limit: state.limits.max_diagnostic_source_depth,
                    },
                )?;
                state
                    .prepare_diagnostic(
                        source,
                        SnapshotMetadata {
                            instance_id: None,
                            descriptor: live.descriptor,
                            code: live.code,
                            tags: live.tags,
                            severity: live.severity,
                        },
                        related_depth,
                        next_depth,
                        Some(DiagnosticRelation::DiagnosticSource),
                        active_diagnostics,
                    )
                    .map(Box::new)
            })
            .transpose()?;

        Ok(DiagnosticSnapshot {
            instance_id: metadata.instance_id,
            code,
            descriptor: metadata.descriptor,
            tags: metadata.tags.to_vec(),
            severity: metadata.severity,
            message,
            labels,
            notes,
            suggestions,
            causes,
            diagnostic_source,
            related,
            contexts: Vec::new(),
        })
    }

    fn prepare_causes(
        &mut self,
        first: Option<&(dyn Error + 'static)>,
    ) -> Result<Vec<OwnedCause>, PrepareError> {
        let mut causes = Vec::new();
        let mut active: Vec<*const ()> = Vec::new();
        let mut current = first;
        let mut depth = 1usize;
        while let Some(cause) = current {
            if depth > self.limits.max_cause_depth {
                return Err(PrepareError::CauseDepthExceeded {
                    depth,
                    limit: self.limits.max_cause_depth,
                });
            }
            let pointer = (cause as *const dyn Error).cast::<()>();
            if active.iter().any(|candidate| core::ptr::eq(*candidate, pointer)) {
                return Err(PrepareError::CauseCycle { depth });
            }
            active.push(pointer);
            self.add_item()?;
            let message = self
                .capture_text(PreparationItemKind::Cause, |out| write!(out, "{cause}"))
                .map_err(|error| match error {
                    CaptureError::Formatting => PrepareError::CauseFormatting { depth },
                    CaptureError::Preparation(error) => error,
                })?;
            causes.push(OwnedCause { message });
            current = cause.source();
            if current.is_some() {
                depth = depth.checked_add(1).ok_or(PrepareError::CauseDepthExceeded {
                    depth: usize::MAX,
                    limit: self.limits.max_cause_depth,
                })?;
            }
        }
        Ok(causes)
    }

    fn add_item(&mut self) -> Result<(), PrepareError> {
        let attempted = self.items.checked_add(1).ok_or(PrepareError::ItemLimitExceeded {
            attempted: usize::MAX,
            limit: self.limits.max_total_items,
        })?;
        if attempted > self.limits.max_total_items {
            return Err(PrepareError::ItemLimitExceeded {
                attempted,
                limit: self.limits.max_total_items,
            });
        }
        self.items = attempted;
        Ok(())
    }

    fn capture_text(
        &mut self,
        kind: PreparationItemKind,
        write: impl FnOnce(&mut dyn fmt::Write) -> fmt::Result,
    ) -> Result<String, CaptureError> {
        let mut writer = BoundedTextWriter {
            output: String::new(),
            kind,
            item_limit: self.limits.max_item_text_bytes,
            total_start: self.text_bytes,
            total_limit: self.limits.max_total_text_bytes,
            error: None,
        };
        if write(&mut writer).is_err() {
            return Err(writer.error.map_or(CaptureError::Formatting, CaptureError::Preparation));
        }
        self.text_bytes = self
            .text_bytes
            .checked_add(writer.output.len())
            .expect("the bounded writer rejected cumulative text overflow");
        Ok(writer.output)
    }

    fn capture_exact_text(
        &mut self,
        kind: PreparationItemKind,
        text: &str,
    ) -> Result<String, PrepareError> {
        self.capture_text(kind, |out| out.write_str(text)).map_err(|error| match error {
            CaptureError::Formatting => PrepareError::VisitorFormatting { item: kind },
            CaptureError::Preparation(error) => error,
        })
    }
}

enum CaptureError {
    Formatting,
    Preparation(PrepareError),
}

impl From<PrepareError> for CaptureError {
    fn from(error: PrepareError) -> Self {
        Self::Preparation(error)
    }
}

struct BoundedTextWriter {
    output: String,
    kind: PreparationItemKind,
    item_limit: usize,
    total_start: usize,
    total_limit: usize,
    error: Option<PrepareError>,
}

impl fmt::Write for BoundedTextWriter {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let Some(item_bytes) = self.output.len().checked_add(text.len()) else {
            self.error = Some(PrepareError::ItemTooLarge {
                item: self.kind,
                bytes: usize::MAX,
                limit: self.item_limit,
            });
            return Err(fmt::Error);
        };
        if item_bytes > self.item_limit {
            self.error = Some(PrepareError::ItemTooLarge {
                item: self.kind,
                bytes: item_bytes,
                limit: self.item_limit,
            });
            return Err(fmt::Error);
        }
        let Some(total_bytes) = self.total_start.checked_add(item_bytes) else {
            self.error = Some(PrepareError::TextLimitExceeded {
                attempted: usize::MAX,
                limit: self.total_limit,
            });
            return Err(fmt::Error);
        };
        if total_bytes > self.total_limit {
            self.error = Some(PrepareError::TextLimitExceeded {
                attempted: total_bytes,
                limit: self.total_limit,
            });
            return Err(fmt::Error);
        }
        self.output.push_str(text);
        Ok(())
    }
}

struct SnapshotVisitor<'a, 'diagnostic> {
    state: &'a mut PreparationState,
    active_diagnostics: &'diagnostic [&'diagnostic dyn Diagnostic],
    labels: Vec<OwnedLabel>,
    notes: Vec<OwnedNote>,
    suggestions: Vec<OwnedSuggestion>,
    related: Vec<DiagnosticSnapshot>,
    related_depth: usize,
    diagnostic_source_depth: usize,
    error: Option<PrepareError>,
}

impl SnapshotVisitor<'_, '_> {
    fn capture_arguments(
        &mut self,
        kind: PreparationItemKind,
        arguments: fmt::Arguments<'_>,
    ) -> Result<String, PrepareError> {
        self.state
            .capture_text(kind, |out| out.write_fmt(arguments))
            .map_err(|error| match error {
                CaptureError::Formatting => PrepareError::VisitorFormatting { item: kind },
                CaptureError::Preparation(error) => error,
            })
    }

    fn fail(&mut self, result: Result<(), PrepareError>) {
        if let Err(error) = result
            && self.error.is_none()
        {
            self.error = Some(error);
        }
    }
}

impl VisitDiagnostic for SnapshotVisitor<'_, '_> {
    fn label(&mut self, label: Label<'_>) {
        if self.error.is_some() {
            return;
        }
        // Unknown and synthetic spans deliberately carry no resolvable source provenance. They
        // are useful sentinels in diagnostics that may or may not have source context, but must
        // not turn an otherwise renderable message into a missing-source failure.
        if label.span.is_unknown() || label.span.is_synthetic() {
            return;
        }
        let result = (|| {
            self.state.add_item()?;
            let message = label
                .message
                .map(|message| self.capture_arguments(PreparationItemKind::LabelMessage, message))
                .transpose()?;
            self.labels.push(OwnedLabel {
                span: label.span,
                style: label.style,
                message,
            });
            Ok(())
        })();
        self.fail(result);
    }

    fn note(&mut self, note: Note<'_>) {
        if self.error.is_some() {
            return;
        }
        let result = (|| {
            self.state.add_item()?;
            let message = self.capture_arguments(PreparationItemKind::NoteMessage, note.message)?;
            self.notes.push(OwnedNote {
                kind: note.kind,
                message,
            });
            Ok(())
        })();
        self.fail(result);
    }

    fn suggestion(&mut self, suggestion: Suggestion<'_>) {
        if self.error.is_some() {
            return;
        }
        let index = self.suggestions.len();
        let result = (|| {
            self.state.add_item()?;
            let message =
                self.capture_arguments(PreparationItemKind::SuggestionMessage, suggestion.message)?;
            let mut edit_visitor = SnapshotEditVisitor {
                state: self.state,
                edits: Vec::new(),
                error: None,
            };
            suggestion.edits.visit_text_edits(&mut edit_visitor);
            if let Some(error) = edit_visitor.error {
                return Err(error);
            }
            let edits = edit_visitor.edits;
            if edits.is_empty() {
                return Err(PrepareError::EmptySuggestion { index });
            }
            self.suggestions.push(OwnedSuggestion {
                message,
                applicability: suggestion.applicability,
                edits,
            });
            Ok(())
        })();
        self.fail(result);
    }

    fn related(&mut self, diagnostic: &dyn Diagnostic) {
        if self.error.is_some() {
            return;
        }
        let live = live_metadata(diagnostic);
        let next_depth = match self.related_depth.checked_add(1) {
            Some(depth) => depth,
            None => {
                self.error = Some(PrepareError::RelatedDepthExceeded {
                    depth: usize::MAX,
                    limit: self.state.limits.max_related_depth,
                });
                return;
            }
        };
        let result = self
            .state
            .prepare_diagnostic(
                diagnostic,
                SnapshotMetadata {
                    instance_id: None,
                    descriptor: live.descriptor,
                    code: live.code,
                    tags: live.tags,
                    severity: live.severity,
                },
                next_depth,
                self.diagnostic_source_depth,
                Some(DiagnosticRelation::Related),
                self.active_diagnostics,
            )
            .map(|snapshot| self.related.push(snapshot));
        self.fail(result);
    }
}

struct SnapshotEditVisitor<'a> {
    state: &'a mut PreparationState,
    edits: Vec<OwnedTextEdit>,
    error: Option<PrepareError>,
}

impl VisitTextEdit for SnapshotEditVisitor<'_> {
    fn text_edit(&mut self, edit: TextEdit<'_>) {
        if self.error.is_some() {
            return;
        }
        let result = (|| {
            self.state.add_item()?;
            let replacement = self
                .state
                .capture_text(PreparationItemKind::Replacement, |out| {
                    out.write_fmt(edit.replacement)
                })
                .map_err(|error| match error {
                    CaptureError::Formatting => PrepareError::VisitorFormatting {
                        item: PreparationItemKind::Replacement,
                    },
                    CaptureError::Preparation(error) => error,
                })?;
            self.edits.push(OwnedTextEdit {
                span: edit.span,
                replacement,
            });
            Ok(())
        })();
        if let Err(error) = result {
            self.error = Some(error);
        }
    }
}

fn live_metadata(diagnostic: &dyn Diagnostic) -> DiagnosticMetadata<'_> {
    DiagnosticMetadata {
        descriptor: diagnostic.descriptor(),
        code: diagnostic.code(),
        severity: diagnostic.severity(),
        tags: diagnostic.tags(),
    }
}
