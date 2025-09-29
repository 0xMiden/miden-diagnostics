use crate::*;

/// Constructs an in-flight diagnostic using the builder pattern
pub struct InFlightDiagnostic<'h> {
    handler: &'h DiagnosticsHandler,
    file_id: Option<SourceId>,
    diagnostic: Diagnostic,
    severity: Severity,
}
impl<'h> InFlightDiagnostic<'h> {
    pub(crate) fn new(handler: &'h DiagnosticsHandler, severity: Severity) -> Self {
        Self {
            handler,
            file_id: None,
            diagnostic: Diagnostic::new(severity),
            severity,
        }
    }

    /// Returns the severity level of this diagnostic
    pub fn severity(&self) -> Severity {
        self.severity
    }

    /// Returns whether this diagnostic should be generated
    /// with verbose detail. Intended to be used when building
    /// diagnostics in-flight by formatting functions which do
    /// not know what the current diagnostic configuration is
    pub fn verbose(&self) -> bool {
        use crate::term::DisplayStyle;
        matches!(self.handler.display.display_style, DisplayStyle::Rich)
    }

    /// Sets the current source file to which this diagnostic applies
    pub fn set_source_file(mut self, filename: impl Into<FileName>) -> Self {
        let filename = filename.into();
        let file_id = self.handler.codemap.get_file_id(&filename);
        self.file_id = file_id;
        self
    }

    /// Sets the diagnostic message to `message`
    pub fn with_message(mut self, message: impl ToString) -> Self {
        self.diagnostic.message = message.to_string();
        self
    }

    /// Adds a primary label for `span` to this diagnostic, with no label message.
    pub fn with_primary_span(mut self, span: SourceSpan) -> Self {
        self.diagnostic
            .labels
            .push(Label::primary(span.source_id(), span));
        self
    }

    /// Adds a primary label for `span` to this diagnostic, with the given message
    ///
    /// A primary label is one which should be rendered as the relevant source code
    /// at which a diagnostic originates. Secondary labels are used for related items
    /// involved in the diagnostic.
    pub fn with_primary_label(mut self, span: SourceSpan, message: impl ToString) -> Self {
        self.diagnostic
            .labels
            .push(Label::primary(span.source_id(), span).with_message(message.to_string()));
        self
    }

    /// Adds a secondary label for `span` to this diagnostic, with the given message
    ///
    /// A secondary label is used to point out related items in the source code which
    /// are relevant to the diagnostic, but which are not themselves the point at which
    /// the diagnostic originates.
    pub fn with_secondary_label(mut self, span: SourceSpan, message: impl ToString) -> Self {
        self.diagnostic
            .labels
            .push(Label::secondary(span.source_id(), span).with_message(message.to_string()));
        self
    }

    /// Like `with_primary_label`, but rather than a [SourceSpan], it accepts a
    /// line and column number, which will be mapped to an appropriate span by
    /// the [CodeMap].
    pub fn with_primary_label_line_and_col(
        self,
        line: u32,
        column: u32,
        message: Option<String>,
    ) -> Self {
        let file_id = self.file_id;
        self.with_label_and_file_id(LabelStyle::Primary, file_id, line, column, message)
    }

    /// This is a lower-level function for adding labels to diagnostics, providing
    /// full control over its style, content, and location in the source code.
    pub fn with_label(
        self,
        style: LabelStyle,
        filename: Option<FileName>,
        line: u32,
        column: u32,
        message: Option<String>,
    ) -> Self {
        if let Some(name) = filename {
            let id = self.handler.lookup_file_id(name);
            self.with_label_and_file_id(style, id, line, column, message)
        } else {
            self
        }
    }

    fn with_label_and_file_id(
        mut self,
        style: LabelStyle,
        file_id: Option<SourceId>,
        line: u32,
        _column: u32,
        message: Option<String>,
    ) -> Self {
        if let Some(id) = file_id {
            let source_file = self.handler.codemap.get(id).unwrap();
            let line_index = (line - 1).into();
            let span = source_file
                .line_span(line_index)
                .expect("invalid line index");
            let label = if let Some(msg) = message {
                Label::new(style, id, span).with_message(msg)
            } else {
                Label::new(style, id, span)
            };
            self.diagnostic.labels.push(label);
            self
        } else {
            self
        }
    }

    /// Adds a note to the diagnostic
    ///
    /// Notes are used for explaining general concepts or suggestions
    /// related to a diagnostic, and are not associated with any particular
    /// source location. They are always rendered after the other diagnostic
    /// content.
    pub fn with_note(mut self, note: impl ToString) -> Self {
        self.diagnostic.notes.push(note.to_string());
        self
    }

    /// Like `with_note`, but is intended for use cases where the
    /// fluent/builder pattern used here is cumbersome.
    pub fn add_note(&mut self, note: impl ToString) {
        self.diagnostic.notes.push(note.to_string());
    }

    /// Consume this [InFlightDiagnostic] and extract the underlying [Diagnostic]
    pub fn take(self) -> Diagnostic {
        self.diagnostic
    }

    /// Emit the underlying [Diagnostic] via the [DiagnosticsHandler]
    pub fn emit(self) {
        self.handler.emit(self.diagnostic);
    }
}
