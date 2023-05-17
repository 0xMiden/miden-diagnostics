use std::io::Write;
use std::ops::Deref;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::term::termcolor::{Color, ColorSpec, WriteColor};
use crate::*;

/// [DiagnosticsHandler] acts as the nexus point for configuring and
/// emitting diagnostics. It puts together many of the pieces provided
/// by this crate to provide a useful and convenient interface for
/// handling diagnostics throughout a compiler.
///
/// In order to construct a [DiagnosticsHandler], you will need a
/// [CodeMap], an [Emitter], and a [DiagnosticsConfig] describing
/// how the handler should behave.
///
/// [DiagnosticsHandler] is a thread-safe structure, and is intended
/// to be passed around freely as needed throughout your project.
pub struct DiagnosticsHandler {
    emitter: Arc<dyn Emitter>,
    pub(crate) codemap: Arc<CodeMap>,
    err_count: AtomicUsize,
    verbosity: Verbosity,
    warnings_as_errors: bool,
    no_warn: bool,
    silent: bool,
    pub(crate) display: crate::term::Config,
}

// We can safely implement these traits for DiagnosticsHandler,
// as the only two non-atomic fields are read-only after creation
unsafe impl Send for DiagnosticsHandler {}
unsafe impl Sync for DiagnosticsHandler {}

impl DiagnosticsHandler {
    /// Create a new [DiagnosticsHandler] from the given [DiagnosticsConfig],
    /// [CodeMap], and [Emitter] implementation.
    pub fn new(
        config: DiagnosticsConfig,
        codemap: Arc<CodeMap>,
        emitter: Arc<dyn Emitter>,
    ) -> Self {
        let no_warn = config.no_warn || config.verbosity > Verbosity::Warning;
        Self {
            emitter,
            codemap,
            err_count: AtomicUsize::new(0),
            verbosity: config.verbosity,
            warnings_as_errors: config.warnings_as_errors,
            no_warn,
            silent: config.verbosity == Verbosity::Silent,
            display: config.display,
        }
    }

    /// Get the [SourceId] corresponding to the given `filename`
    pub fn lookup_file_id(&self, filename: impl Into<FileName>) -> Option<SourceId> {
        let filename = filename.into();
        self.codemap.get_file_id(&filename)
    }

    /// Returns true if the [DiagnosticsHandler] has emitted any error diagnostics
    pub fn has_errors(&self) -> bool {
        self.err_count.load(Ordering::Relaxed) > 0
    }

    /// Triggers a panic if the [DiagnosticsHandler] has emitted any error diagnostics
    #[track_caller]
    pub fn abort_if_errors(&self) {
        if self.has_errors() {
            FatalError.raise();
        }
    }

    /// Emits an error message and produces a FatalError object
    /// which can be used to terminate execution immediately
    pub fn fatal(&self, err: impl ToString) -> FatalError {
        self.error(err);
        FatalError
    }

    /// Report an error diagnostic
    pub fn error(&self, error: impl ToString) {
        let diagnostic = Diagnostic::error().with_message(error.to_string());
        self.emit(diagnostic);
    }

    /// Report a warning diagnostic
    ///
    /// If `warnings_as_errors` is set, it produces an error diagnostic instead.
    pub fn warn(&self, warning: impl ToString) {
        if self.warnings_as_errors {
            return self.error(warning);
        }
        let diagnostic = Diagnostic::warning().with_message(warning.to_string());
        self.emit(diagnostic);
    }

    /// Emits an informational diagnostic
    pub fn info(&self, message: impl ToString) {
        if self.verbosity > Verbosity::Info {
            return;
        }
        let info_color = self.display.styles.header(Severity::Help);
        let mut buffer = self.emitter.buffer();
        buffer.set_color(info_color).ok();
        buffer.write_all(b"info").unwrap();
        buffer.set_color(&self.display.styles.header_message).ok();
        writeln!(&mut buffer, ": {}", message.to_string()).unwrap();
        buffer.reset().ok();
        self.emitter.print(buffer).unwrap();
    }

    /// Emits a debug diagnostic
    pub fn debug(&self, message: impl ToString) {
        if self.verbosity > Verbosity::Debug {
            return;
        }
        let mut debug_color = self.display.styles.header_message.clone();
        debug_color.set_fg(Some(Color::Blue));
        let mut buffer = self.emitter.buffer();
        buffer.set_color(&debug_color).ok();
        buffer.write_all(b"debug").unwrap();
        buffer.set_color(&self.display.styles.header_message).ok();
        writeln!(&mut buffer, ": {}", message.to_string()).unwrap();
        buffer.reset().ok();
        self.emitter.print(buffer).unwrap();
    }

    /// Emits a note diagnostic
    pub fn note(&self, message: impl ToString) {
        if self.verbosity > Verbosity::Info {
            return;
        }
        self.emit(Diagnostic::note().with_message(message.to_string()));
    }

    /// Prints a warning-like message with the given prefix
    ///
    /// NOTE: This does not get promoted to an error if warnings-as-errors is set,
    /// as it is intended for informational purposes, not issues with the code being compiled
    pub fn notice(&self, prefix: &str, message: impl ToString) {
        if self.verbosity > Verbosity::Info {
            return;
        }
        self.write_prefixed(
            self.display.styles.header(Severity::Warning),
            prefix,
            message,
        );
    }

    /// Prints a success message with the given prefix
    pub fn success(&self, prefix: &str, message: impl ToString) {
        if self.silent {
            return;
        }
        self.write_prefixed(self.display.styles.header(Severity::Note), prefix, message);
    }

    /// Prints an error message with the given prefix
    pub fn failed(&self, prefix: &str, message: impl ToString) {
        self.err_count.fetch_add(1, Ordering::Relaxed);
        self.write_prefixed(self.display.styles.header(Severity::Error), prefix, message);
    }

    fn write_prefixed(&self, color: &ColorSpec, prefix: &str, message: impl ToString) {
        let mut buffer = self.emitter.buffer();
        buffer.set_color(color).ok();
        write!(&mut buffer, "{:>12} ", prefix).unwrap();
        buffer.reset().ok();
        let message = message.to_string();
        buffer.write_all(message.as_bytes()).unwrap();
        self.emitter.print(buffer).unwrap();
    }

    /// Starts building an [InFlightDiagnostic] for rich compiler diagnostics.
    ///
    /// The caller is responsible for dropping/emitting the diagnostic using the
    /// [InFlightDiagnostic] API.
    pub fn diagnostic(&self, severity: Severity) -> InFlightDiagnostic<'_> {
        InFlightDiagnostic::new(self, severity)
    }

    /// Emits the given diagnostic
    #[inline(always)]
    pub fn emit(&self, diagnostic: impl ToDiagnostic) {
        if self.silent {
            return;
        }

        let mut diagnostic = diagnostic.to_diagnostic();
        match diagnostic.severity {
            Severity::Note if self.verbosity > Verbosity::Info => return,
            Severity::Warning if self.no_warn => return,
            Severity::Warning if self.warnings_as_errors => {
                diagnostic.severity = Severity::Error;
            }
            _ => (),
        }

        if diagnostic.severity == Severity::Error {
            self.err_count.fetch_add(1, Ordering::Relaxed);
        }

        let mut buffer = self.emitter.buffer();
        crate::term::emit(
            &mut buffer,
            &self.display,
            self.codemap.deref(),
            &diagnostic,
        )
        .unwrap();
        self.emitter.print(buffer).unwrap();
    }
}
