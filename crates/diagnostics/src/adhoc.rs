use alloc::{boxed::Box, fmt::format, string::String, vec::Vec};
use core::{error::Error, fmt};

use crate::{
    Applicability, Diagnostic, DiagnosticCodeOwned, DiagnosticCodeRef, DiagnosticDescriptor,
    DiagnosticTag, Label, LabelStyle, Note, OwnedDiagnostic, OwnedLabel, OwnedNote,
    OwnedSuggestion, OwnedTextEdit, Severity, Suggestion, VisitDiagnostic,
};

#[doc(hidden)]
pub fn format_arguments(arguments: fmt::Arguments<'_>) -> String {
    format(arguments)
}

/// The concrete occurrence returned by [`crate::diagnostic!`].
///
/// This type is public so an exported expression macro can return it across a
/// crate boundary. Its construction API is intentionally hidden; use
/// `diagnostic!` instead.
#[doc(hidden)]
pub struct AdHocDiagnostic {
    descriptor: Option<&'static DiagnosticDescriptor>,
    code: Option<DiagnosticCodeOwned>,
    severity_override: Option<Severity>,
    message: String,
    labels: Vec<OwnedLabel>,
    notes: Vec<OwnedNote>,
    suggestions: Vec<OwnedSuggestion>,
    cause: Option<Box<dyn Error + Send + Sync>>,
    diagnostic_source: Option<OwnedDiagnostic>,
    related: Vec<OwnedDiagnostic>,
}

impl AdHocDiagnostic {
    #[doc(hidden)]
    pub fn new(message: fmt::Arguments<'_>) -> Self {
        Self {
            descriptor: None,
            code: None,
            severity_override: None,
            message: format_arguments(message),
            labels: Vec::new(),
            notes: Vec::new(),
            suggestions: Vec::new(),
            cause: None,
            diagnostic_source: None,
            related: Vec::new(),
        }
    }

    #[doc(hidden)]
    pub fn with_descriptor(mut self, descriptor: &'static DiagnosticDescriptor) -> Self {
        self.descriptor = Some(descriptor);
        self
    }

    #[doc(hidden)]
    pub fn with_canonical_code(mut self, canonical: &str) -> Self {
        let (namespace, code) = canonical
            .split_once('/')
            .expect("diagnostic! validates literal canonical codes");
        self.code = Some(DiagnosticCodeOwned::new(namespace, code));
        self
    }

    #[doc(hidden)]
    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity_override = Some(severity);
        self
    }

    #[doc(hidden)]
    pub fn push_label(
        &mut self,
        span: crate::SourceSpan,
        style: LabelStyle,
        message: Option<fmt::Arguments<'_>>,
    ) {
        self.labels.push(OwnedLabel {
            span,
            style,
            message: message.map(format_arguments),
        });
    }

    #[doc(hidden)]
    pub fn push_note(&mut self, kind: crate::NoteKind, message: fmt::Arguments<'_>) {
        self.notes.push(OwnedNote {
            kind,
            message: format_arguments(message),
        });
    }

    #[doc(hidden)]
    pub fn push_suggestion(
        &mut self,
        message: fmt::Arguments<'_>,
        applicability: Applicability,
        edits: Vec<OwnedTextEdit>,
    ) {
        self.suggestions.push(OwnedSuggestion {
            message: format_arguments(message),
            applicability,
            edits,
        });
    }

    #[doc(hidden)]
    pub fn set_cause<E>(&mut self, cause: E)
    where
        E: Error + Send + Sync + 'static,
    {
        self.cause = Some(Box::new(cause));
    }

    #[doc(hidden)]
    pub fn set_diagnostic_source<D>(&mut self, diagnostic: D)
    where
        D: Diagnostic + Send + Sync + 'static,
    {
        self.diagnostic_source = Some(OwnedDiagnostic::new(diagnostic));
    }

    #[doc(hidden)]
    pub fn push_related<D>(&mut self, diagnostic: D)
    where
        D: Diagnostic + Send + Sync + 'static,
    {
        self.related.push(OwnedDiagnostic::new(diagnostic));
    }
}

impl Diagnostic for AdHocDiagnostic {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str(&self.message)
    }

    fn descriptor(&self) -> Option<&'static DiagnosticDescriptor> {
        self.descriptor
    }

    fn code(&self) -> Option<DiagnosticCodeRef<'_>> {
        self.descriptor
            .map(|descriptor| descriptor.code.as_ref())
            .or_else(|| self.code.as_ref().map(DiagnosticCodeOwned::as_ref))
    }

    fn severity(&self) -> Severity {
        self.severity_override
            .or_else(|| self.descriptor.map(|descriptor| descriptor.default_severity))
            .unwrap_or(Severity::Error)
    }

    fn tags(&self) -> &[DiagnosticTag] {
        self.descriptor.map_or(&[], |descriptor| descriptor.tags)
    }

    fn visit(&self, visitor: &mut dyn VisitDiagnostic) {
        for label in &self.labels {
            match &label.message {
                Some(message) => visitor.label(Label {
                    span: label.span,
                    style: label.style,
                    message: Some(format_args!("{message}")),
                }),
                None => visitor.label(Label {
                    span: label.span,
                    style: label.style,
                    message: None,
                }),
            }
        }
        for note in &self.notes {
            visitor.note(Note {
                kind: note.kind,
                message: format_args!("{}", note.message),
            });
        }
        for suggestion in &self.suggestions {
            visitor.suggestion(Suggestion {
                message: format_args!("{}", suggestion.message),
                applicability: suggestion.applicability,
                edits: &suggestion.edits,
            });
        }
        for related in &self.related {
            visitor.related(related.as_diagnostic());
        }
    }

    fn cause(&self) -> Option<&(dyn Error + 'static)> {
        self.cause.as_deref().map(|cause| cause as &dyn Error)
    }

    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        self.diagnostic_source.as_ref().map(OwnedDiagnostic::as_diagnostic)
    }
}

impl fmt::Debug for AdHocDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdHocDiagnostic")
            .field("descriptor", &self.descriptor)
            .field("code", &self.code)
            .field("severity", &self.severity())
            .field("message", &self.message)
            .field("labels", &self.labels)
            .field("notes", &self.notes)
            .field("suggestions", &self.suggestions)
            .field("has_cause", &self.cause.is_some())
            .field("has_diagnostic_source", &self.diagnostic_source.is_some())
            .field("related_count", &self.related.len())
            .finish()
    }
}

#[doc(hidden)]
pub const fn validate_canonical_code(canonical: &str) {
    let bytes = canonical.as_bytes();
    let mut index = 0;
    let mut separator = None;
    while index < bytes.len() {
        if bytes[index] == b'/' {
            assert!(separator.is_none(), "diagnostic code must contain exactly one `/`");
            separator = Some(index);
        }
        index += 1;
    }
    let separator = separator.expect("diagnostic code must be `namespace/code`");
    assert!(separator > 0, "diagnostic namespace must not be empty");
    assert!(separator + 1 < bytes.len(), "diagnostic code component must not be empty");
}
