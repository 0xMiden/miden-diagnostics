use alloc::{format, string::String};
use core::{error::Error, fmt};

use crate::{
    AnnotateRenderer, PreparedDiagnostic, PreparedSet, Severity,
    render::{normalize_display_text, sanitize_rendered_output},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmissionStatus {
    Rendered,
    Degraded,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EmissionSummary {
    pub emitted: usize,
    pub degraded: usize,
}

impl EmissionSummary {
    fn record(&mut self, status: EmissionStatus) {
        self.emitted = self.emitted.saturating_add(1);
        if status == EmissionStatus::Degraded {
            self.degraded = self.degraded.saturating_add(1);
        }
    }
}

#[derive(Debug)]
pub struct EmissionFailure<E> {
    pub error: E,
    pub completed: EmissionSummary,
}

impl<E: fmt::Display> fmt::Display for EmissionFailure<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "diagnostic emission failed after {} records: {}",
            self.completed.emitted, self.error
        )
    }
}

impl<E> Error for EmissionFailure<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}

/// Object-safe presentation boundary. Emission never decides failure policy.
pub trait Emitter {
    type Error;

    fn emit(&mut self, diagnostic: &PreparedDiagnostic<'_>) -> Result<EmissionStatus, Self::Error>;

    fn emit_set(
        &mut self,
        diagnostics: &PreparedSet<'_>,
    ) -> Result<EmissionSummary, EmissionFailure<Self::Error>> {
        let mut completed = EmissionSummary::default();
        for diagnostic in diagnostics {
            match self.emit(diagnostic) {
                Ok(status) => completed.record(status),
                Err(error) => return Err(EmissionFailure { error, completed }),
            }
        }
        Ok(completed)
    }
}

pub struct FmtEmitter<W: fmt::Write> {
    writer: W,
    renderer: AnnotateRenderer,
}

impl<W: fmt::Write> FmtEmitter<W> {
    pub const fn new(writer: W, renderer: AnnotateRenderer) -> Self {
        Self { writer, renderer }
    }

    pub const fn renderer(&self) -> &AnnotateRenderer {
        &self.renderer
    }

    pub fn renderer_mut(&mut self) -> &mut AnnotateRenderer {
        &mut self.renderer
    }

    pub const fn writer(&self) -> &W {
        &self.writer
    }

    pub fn writer_mut(&mut self) -> &mut W {
        &mut self.writer
    }

    pub fn into_inner(self) -> W {
        self.writer
    }

    fn record(&self, diagnostic: &PreparedDiagnostic<'_>) -> (String, EmissionStatus) {
        render_or_degrade(&self.renderer, diagnostic)
    }
}

impl<W: fmt::Write> Emitter for FmtEmitter<W> {
    type Error = fmt::Error;

    fn emit(&mut self, diagnostic: &PreparedDiagnostic<'_>) -> Result<EmissionStatus, Self::Error> {
        let (record, status) = self.record(diagnostic);
        self.writer.write_str(&record)?;
        self.writer.write_char('\n')?;
        Ok(status)
    }
}

#[cfg(feature = "std")]
#[derive(Debug)]
pub enum IoEmissionError {
    Write(std::io::Error),
    Flush(std::io::Error),
}

#[cfg(feature = "std")]
impl fmt::Display for IoEmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Write(error) => write!(formatter, "diagnostic write failed: {error}"),
            Self::Flush(error) => write!(formatter, "diagnostic flush failed: {error}"),
        }
    }
}

#[cfg(feature = "std")]
impl Error for IoEmissionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Write(error) | Self::Flush(error) => Some(error),
        }
    }
}

#[cfg(feature = "std")]
pub struct IoEmitter<W: std::io::Write> {
    writer: W,
    renderer: AnnotateRenderer,
}

#[cfg(feature = "std")]
impl<W: std::io::Write> IoEmitter<W> {
    pub const fn new(writer: W, renderer: AnnotateRenderer) -> Self {
        Self { writer, renderer }
    }

    pub const fn renderer(&self) -> &AnnotateRenderer {
        &self.renderer
    }

    pub fn renderer_mut(&mut self) -> &mut AnnotateRenderer {
        &mut self.renderer
    }

    pub const fn writer(&self) -> &W {
        &self.writer
    }

    pub fn writer_mut(&mut self) -> &mut W {
        &mut self.writer
    }

    pub fn into_inner(self) -> W {
        self.writer
    }

    fn write_record(
        &mut self,
        diagnostic: &PreparedDiagnostic<'_>,
    ) -> Result<EmissionStatus, IoEmissionError> {
        let (record, status) = render_or_degrade(&self.renderer, diagnostic);
        self.writer.write_all(record.as_bytes()).map_err(IoEmissionError::Write)?;
        self.writer.write_all(b"\n").map_err(IoEmissionError::Write)?;
        Ok(status)
    }
}

#[cfg(feature = "std")]
impl<W: std::io::Write> Emitter for IoEmitter<W> {
    type Error = IoEmissionError;

    fn emit(&mut self, diagnostic: &PreparedDiagnostic<'_>) -> Result<EmissionStatus, Self::Error> {
        let status = self.write_record(diagnostic)?;
        self.writer.flush().map_err(IoEmissionError::Flush)?;
        Ok(status)
    }

    fn emit_set(
        &mut self,
        diagnostics: &PreparedSet<'_>,
    ) -> Result<EmissionSummary, EmissionFailure<Self::Error>> {
        let mut completed = EmissionSummary::default();
        for diagnostic in diagnostics {
            match self.write_record(diagnostic) {
                Ok(status) => completed.record(status),
                Err(error) => return Err(EmissionFailure { error, completed }),
            }
        }
        if let Err(error) = self.writer.flush() {
            return Err(EmissionFailure {
                error: IoEmissionError::Flush(error),
                completed,
            });
        }
        Ok(completed)
    }
}

pub(crate) fn render_or_degrade(
    renderer: &AnnotateRenderer,
    diagnostic: &PreparedDiagnostic<'_>,
) -> (String, EmissionStatus) {
    match renderer.render(diagnostic) {
        Ok(output) => (output, EmissionStatus::Rendered),
        Err(error) => (degraded_fallback(diagnostic, error.category()), EmissionStatus::Degraded),
    }
}

fn degraded_fallback(diagnostic: &PreparedDiagnostic<'_>, category: &'static str) -> String {
    let snapshot = &diagnostic.snapshot;
    let severity = severity_name(snapshot.severity);
    let code = snapshot
        .code
        .as_ref()
        .map(|code| normalize_display_text(&format!("{}/{}", code.namespace, code.code)));
    let message = normalize_display_text(&snapshot.message);
    let title = match code {
        Some(code) => format!("{severity}[{code}]: {message}"),
        None => format!("{severity}: {message}"),
    };
    let fallback = format!("{title}\nnote: diagnostic rendering degraded: {category}");
    sanitize_rendered_output(&fallback, false, false, &[])
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
        Severity::Hint => "hint",
    }
}
