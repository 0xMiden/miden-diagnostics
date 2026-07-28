use core::fmt;

use crate::{DiagnosticCodeRef, DiagnosticDescriptor, DiagnosticTag, Severity, SourceSpan};

/// An object-safe semantic diagnostic occurrence.
pub trait Diagnostic: fmt::Debug {
    /// Write the message for this diagnostic to `out`
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result;

    /// An optional descriptor for this diagnostic
    fn descriptor(&self) -> Option<&'static DiagnosticDescriptor> {
        None
    }

    /// An optional diagnostic code
    fn code(&self) -> Option<DiagnosticCodeRef<'_>> {
        self.descriptor().map(|descriptor| descriptor.code.as_ref())
    }

    /// The severity of this diagnostic.
    ///
    /// Defaults to `Error` if unimplemented, unless `descriptor` is implemented and provides a
    /// different default severity.
    fn severity(&self) -> Severity {
        self.descriptor()
            .map_or(Severity::Error, |descriptor| descriptor.default_severity)
    }

    /// Tags associated with this diagnostic
    fn tags(&self) -> &[DiagnosticTag] {
        self.descriptor().map_or(&[], |descriptor| descriptor.tags)
    }

    /// Visit the structured children of this diagnostic (e.g. labels, notes, etc.)
    fn visit(&self, _visitor: &mut dyn VisitDiagnostic) {}

    /// Returns the associated [`core::error::Error`] type, if this diagnostic has one
    fn cause(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }

    /// Returns a source [`Diagnostic`] type, if this diagnostic is derived from another
    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        None
    }
}

/// A synchronous visitor over one diagnostic's structured children.
pub trait VisitDiagnostic {
    fn label(&mut self, _label: Label<'_>) {}

    fn note(&mut self, _note: Note<'_>) {}

    fn suggestion(&mut self, _suggestion: Suggestion<'_>) {}

    fn related(&mut self, _diagnostic: &dyn Diagnostic) {}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LabelStyle {
    Primary,
    Context,
}

pub struct Label<'a> {
    pub span: SourceSpan,
    pub style: LabelStyle,
    pub message: Option<fmt::Arguments<'a>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteKind {
    Note,
    Help,
}

pub struct Note<'a> {
    pub kind: NoteKind,
    pub message: fmt::Arguments<'a>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Applicability {
    MachineApplicable,
    MaybeIncorrect,
    HasPlaceholders,
    Unspecified,
}

pub struct Suggestion<'a> {
    pub message: fmt::Arguments<'a>,
    pub applicability: Applicability,
    pub edits: &'a dyn VisitTextEdits,
}

pub struct TextEdit<'a> {
    pub span: SourceSpan,
    pub replacement: fmt::Arguments<'a>,
}

/// A synchronous producer of the edits in one atomic suggestion.
pub trait VisitTextEdits {
    fn visit_text_edits(&self, visitor: &mut dyn VisitTextEdit);
}

/// A synchronous visitor over the edits in one suggestion.
pub trait VisitTextEdit {
    fn text_edit(&mut self, edit: TextEdit<'_>);
}

impl<const N: usize> VisitTextEdits for [TextEdit<'_>; N] {
    fn visit_text_edits(&self, visitor: &mut dyn VisitTextEdit) {
        for edit in self {
            visitor.text_edit(TextEdit {
                span: edit.span,
                replacement: edit.replacement,
            });
        }
    }
}

pub(crate) struct DiagnosticMessage<'a>(pub(crate) &'a dyn Diagnostic);

impl fmt::Display for DiagnosticMessage<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.message(formatter)
    }
}

#[cfg(test)]
mod tests {
    use alloc::{
        rc::Rc,
        string::{String, ToString},
        vec::Vec,
    };
    use core::{
        cell::Cell,
        fmt::{self, Write},
    };

    use super::*;
    use crate::{SourceId, SourceNamespace, TextRange};

    fn span() -> SourceSpan {
        SourceSpan::session(SourceId::new(SourceNamespace(1), 0), TextRange::new(0, 0).unwrap())
    }

    #[derive(Debug)]
    struct Local<'a> {
        prefix: &'a str,
        value: Rc<Cell<u32>>,
    }

    struct CellDisplay<'a>(&'a Cell<u32>);

    impl fmt::Display for CellDisplay<'_> {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.get().fmt(formatter)
        }
    }

    impl Diagnostic for Local<'_> {
        fn message(&self, out: &mut dyn Write) -> fmt::Result {
            write!(out, "{} {}", self.prefix, self.value.get())
        }

        fn visit(&self, visitor: &mut dyn VisitDiagnostic) {
            visitor.label(Label {
                span: span(),
                style: LabelStyle::Primary,
                message: Some(format_args!("label {}", CellDisplay(&self.value))),
            });
            self.value.set(8);
            visitor.note(Note {
                kind: NoteKind::Note,
                message: format_args!("note {}", CellDisplay(&self.value)),
            });
            self.value.set(9);
            let edits = [TextEdit {
                span: span(),
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

    #[derive(Default)]
    struct CollectingVisitor {
        labels: Vec<String>,
        notes: Vec<String>,
        suggestions: Vec<String>,
        replacements: Vec<String>,
    }

    impl VisitDiagnostic for CollectingVisitor {
        fn label(&mut self, label: Label<'_>) {
            self.labels.push(label.message.unwrap().to_string());
        }

        fn note(&mut self, note: Note<'_>) {
            self.notes.push(note.message.to_string());
        }

        fn suggestion(&mut self, suggestion: Suggestion<'_>) {
            self.suggestions.push(suggestion.message.to_string());
            struct CollectEdits<'a>(&'a mut Vec<String>);

            impl VisitTextEdit for CollectEdits<'_> {
                fn text_edit(&mut self, edit: TextEdit<'_>) {
                    self.0.push(edit.replacement.to_string());
                }
            }

            suggestion.edits.visit_text_edits(&mut CollectEdits(&mut self.replacements));
        }
    }

    #[test]
    fn protocols_are_object_safe_and_borrowed_arguments_are_consumed_synchronously() {
        fn accepts_objects(_: &dyn Diagnostic, _: &mut dyn VisitDiagnostic) {}

        let value = Rc::new(Cell::new(7));
        let prefix = String::from("local");
        let diagnostic = Local {
            prefix: &prefix,
            value: Rc::clone(&value),
        };
        let mut visitor = CollectingVisitor::default();
        accepts_objects(&diagnostic, &mut visitor);

        let mut message = String::new();
        diagnostic.message(&mut message).unwrap();
        diagnostic.visit(&mut visitor);
        value.set(99);

        assert_eq!(message, "local 7");
        assert_eq!(visitor.labels, ["label 7"]);
        assert_eq!(visitor.notes, ["note 8"]);
        assert_eq!(visitor.suggestions, ["suggestion 9"]);
        assert_eq!(visitor.replacements, ["replacement 9"]);

        struct RejectWrites;

        impl Write for RejectWrites {
            fn write_str(&mut self, _text: &str) -> fmt::Result {
                Err(fmt::Error)
            }
        }

        assert_eq!(diagnostic.message(&mut RejectWrites), Err(fmt::Error));
    }

    #[derive(Debug)]
    struct Related;

    impl Diagnostic for Related {
        fn message(&self, out: &mut dyn Write) -> fmt::Result {
            out.write_str("related")
        }
    }

    #[derive(Debug)]
    struct Root(Related);

    impl Diagnostic for Root {
        fn message(&self, out: &mut dyn Write) -> fmt::Result {
            out.write_str("root")
        }

        fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
            Some(&self.0)
        }

        fn visit(&self, visitor: &mut dyn VisitDiagnostic) {
            visitor.related(&self.0);
        }
    }

    struct ChannelVisitor {
        related: usize,
    }

    impl VisitDiagnostic for ChannelVisitor {
        fn related(&mut self, _diagnostic: &dyn Diagnostic) {
            self.related += 1;
        }
    }

    #[test]
    fn diagnostic_source_and_related_are_distinct_channels() {
        let root = Root(Related);
        assert_eq!(
            root.diagnostic_source().map(|source| DiagnosticMessage(source).to_string()),
            Some("related".to_string())
        );
        let mut visitor = ChannelVisitor { related: 0 };
        root.visit(&mut visitor);
        assert_eq!(visitor.related, 1);
    }
}
