use alloc::{boxed::Box, string::String, vec::Vec};
use core::{any::Any, fmt};

use crate::{
    AnnotateRenderer, Diagnostic, DiagnosticCodeRef, DiagnosticDescriptor, DiagnosticMetadata,
    DiagnosticTag, LayeredSourceProvider, PreparationLimits, PrepareError, PreparedDiagnostic,
    RenderConfig, RenderError, Severity, SourceProvider,
    diagnostic::DiagnosticMessage,
    emit::render_or_degrade,
    snapshot::{prepare_owned_ref, prepare_owned_ref_with_limits},
    source::EMPTY_SOURCE_PROVIDER,
};

const REPORT_PREPARATION_FALLBACK: &str =
    "error: diagnostic preparation failed; rich output unavailable";

/// A failure while preparing or rendering one owned diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiagnosticRenderError {
    /// Snapshot preparation failed.
    Prepare(PrepareError),
    /// Rendering a prepared snapshot failed.
    Render(RenderError),
}

impl fmt::Display for DiagnosticRenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Prepare(error) => write!(formatter, "diagnostic preparation failed: {error}"),
            Self::Render(error) => write!(formatter, "diagnostic rendering failed: {error}"),
        }
    }
}

impl core::error::Error for DiagnosticRenderError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Prepare(error) => Some(error),
            Self::Render(error) => Some(error),
        }
    }
}

impl From<PrepareError> for DiagnosticRenderError {
    fn from(error: PrepareError) -> Self {
        Self::Prepare(error)
    }
}

impl From<RenderError> for DiagnosticRenderError {
    fn from(error: RenderError) -> Self {
        Self::Render(error)
    }
}

trait ErasedDiagnostic: Diagnostic + Send + Sync + 'static {
    fn diagnostic(&self) -> &dyn Diagnostic;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

impl<T> ErasedDiagnostic for T
where
    T: Diagnostic + Send + Sync + 'static,
{
    fn diagnostic(&self) -> &dyn Diagnostic {
        self
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// An owned report context captured as text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextFrame {
    message: String,
}

impl ContextFrame {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// An owned, transport-safe diagnostic occurrence.
pub struct OwnedDiagnostic {
    inner: Box<dyn ErasedDiagnostic>,
    contexts: Vec<ContextFrame>,
    severity_override: Option<Severity>,
    session_sources: Option<Box<dyn SourceProvider + Send + Sync>>,
    attached_sources: Option<Box<dyn SourceProvider + Send + Sync>>,
}

impl OwnedDiagnostic {
    /// Owns a transport-safe diagnostic.
    ///
    /// Borrowed or local diagnostics remain usable through direct synchronous
    /// `Diagnostic::message` and `Diagnostic::visit`, but they cannot cross
    /// this owned boundary.
    ///
    /// ```compile_fail
    /// extern crate alloc;
    /// use miden_diagnostics::{Diagnostic, OwnedDiagnostic};
    /// use core::fmt;
    ///
    /// #[derive(Debug)]
    /// struct Borrowed<'a>(&'a str);
    /// impl Diagnostic for Borrowed<'_> {
    ///     fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
    ///         out.write_str(self.0)
    ///     }
    /// }
    ///
    /// let text = alloc::string::String::from("local");
    /// let _ = OwnedDiagnostic::new(Borrowed(&text));
    /// ```
    ///
    /// ```compile_fail
    /// extern crate alloc;
    /// use alloc::rc::Rc;
    /// use core::{cell::Cell, fmt};
    /// use miden_diagnostics::{Diagnostic, OwnedDiagnostic};
    ///
    /// #[derive(Debug)]
    /// struct Local(Rc<Cell<u32>>);
    /// impl Diagnostic for Local {
    ///     fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
    ///         write!(out, "{}", self.0.get())
    ///     }
    /// }
    ///
    /// let _ = OwnedDiagnostic::new(Local(Rc::new(Cell::new(1))));
    /// ```
    pub fn new<T>(diagnostic: T) -> Self
    where
        T: Diagnostic + Send + Sync + 'static,
    {
        Self {
            inner: Box::new(diagnostic),
            contexts: Vec::new(),
            severity_override: None,
            session_sources: None,
            attached_sources: None,
        }
    }

    pub fn as_diagnostic(&self) -> &dyn Diagnostic {
        self.inner.diagnostic()
    }

    pub fn descriptor(&self) -> Option<&'static DiagnosticDescriptor> {
        self.as_diagnostic().descriptor()
    }

    pub fn code(&self) -> Option<DiagnosticCodeRef<'_>> {
        self.as_diagnostic().code()
    }

    pub fn tags(&self) -> &[DiagnosticTag] {
        self.as_diagnostic().tags()
    }

    pub fn severity(&self) -> Severity {
        self.severity_override.unwrap_or_else(|| self.as_diagnostic().severity())
    }

    pub const fn severity_override(&self) -> Option<Severity> {
        self.severity_override
    }

    pub fn set_severity_override(&mut self, severity: Option<Severity>) {
        self.severity_override = severity;
    }

    pub fn with_severity_override(mut self, severity: Severity) -> Self {
        self.severity_override = Some(severity);
        self
    }

    pub fn metadata(&self) -> DiagnosticMetadata<'_> {
        DiagnosticMetadata {
            descriptor: self.descriptor(),
            code: self.code(),
            severity: self.severity(),
            tags: self.tags(),
        }
    }

    pub fn contexts(&self) -> &[ContextFrame] {
        &self.contexts
    }

    pub fn push_context(&mut self, context: impl Into<String>) {
        self.contexts.push(ContextFrame::new(context));
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.push_context(context);
        self
    }

    pub fn attach_sources<P>(mut self, sources: P) -> Self
    where
        P: SourceProvider + Send + Sync + 'static,
    {
        self.attached_sources = Some(Box::new(sources));
        self
    }

    /// Retains a session source provider with this diagnostic occurrence.
    ///
    /// This is useful when the operation that allocated session source IDs transfers diagnostics
    /// to a caller without otherwise retaining the operation's source manager. An explicit source
    /// provider passed to [`Self::prepare`] or [`Self::display_with_sources`] takes precedence.
    pub fn attach_session_sources<P>(mut self, sources: P) -> Self
    where
        P: SourceProvider + Send + Sync + 'static,
    {
        self.set_session_sources(sources);
        self
    }

    pub fn set_session_sources<P>(&mut self, sources: P)
    where
        P: SourceProvider + Send + Sync + 'static,
    {
        self.session_sources = Some(Box::new(sources));
    }

    pub fn session_sources(&self) -> Option<&(dyn SourceProvider + Send + Sync + 'static)> {
        self.session_sources.as_deref()
    }

    pub fn attached_sources(&self) -> Option<&(dyn SourceProvider + Send + Sync + 'static)> {
        self.attached_sources.as_deref()
    }

    /// Prepares this diagnostic with its occurrence metadata and source universes.
    ///
    /// `session_sources` resolves [`crate::SourceKey::Session`] spans. Sources
    /// attached to this diagnostic continue to resolve
    /// [`crate::SourceKey::Attached`] spans without fallback between the two
    /// namespaces.
    pub fn prepare<'a>(
        &'a self,
        session_sources: &'a dyn SourceProvider,
    ) -> Result<PreparedDiagnostic<'a>, PrepareError> {
        let snapshot = prepare_owned_ref(self)?;
        Ok(self.prepared(snapshot, session_sources))
    }

    /// Prepares this diagnostic with explicit resource limits.
    pub fn prepare_with_limits<'a>(
        &'a self,
        session_sources: &'a dyn SourceProvider,
        limits: PreparationLimits,
    ) -> Result<PreparedDiagnostic<'a>, PrepareError> {
        let snapshot = prepare_owned_ref_with_limits(self, limits)?;
        Ok(self.prepared(snapshot, session_sources))
    }

    /// Prepares this diagnostic using only sources attached to it.
    ///
    /// Rendering a session span in the returned diagnostic will produce a
    /// missing-source error.
    pub fn prepare_attached(&self) -> Result<PreparedDiagnostic<'_>, PrepareError> {
        let session = self
            .session_sources()
            .map_or(&EMPTY_SOURCE_PROVIDER as &dyn SourceProvider, |sources| sources);
        self.prepare(session)
    }

    /// Prepares this diagnostic using only attached sources and explicit limits.
    pub fn prepare_attached_with_limits(
        &self,
        limits: PreparationLimits,
    ) -> Result<PreparedDiagnostic<'_>, PrepareError> {
        let session = self
            .session_sources()
            .map_or(&EMPTY_SOURCE_PROVIDER as &dyn SourceProvider, |sources| sources);
        self.prepare_with_limits(session, limits)
    }

    /// Returns a rich-formatting adapter using attached sources and portable defaults.
    pub fn display(&self) -> DiagnosticDisplay<'_> {
        let session = self
            .session_sources()
            .map_or(&EMPTY_SOURCE_PROVIDER as &dyn SourceProvider, |sources| sources);
        DiagnosticDisplay::new(self, session)
    }

    /// Returns a rich-formatting adapter with an explicit session source provider.
    ///
    /// Sources attached to this diagnostic are layered with `session_sources`
    /// and remain scoped to attached spans.
    pub fn display_with_sources<'a>(
        &'a self,
        session_sources: &'a dyn SourceProvider,
    ) -> DiagnosticDisplay<'a> {
        DiagnosticDisplay::new(self, session_sources)
    }

    pub fn is<T: 'static>(&self) -> bool {
        let any = self.inner.as_any();
        any.is::<T>() || any.is::<DiagnosticError<T>>()
    }

    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        if let Some(t) = self.inner.as_any().downcast_ref::<T>() {
            Some(t)
        } else {
            self.inner.as_any().downcast_ref::<DiagnosticError<T>>().map(AsRef::as_ref)
        }
    }

    pub fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
        let inner = self.inner.as_any_mut();
        if inner.is::<T>() {
            inner.downcast_mut()
        } else {
            inner.downcast_mut::<DiagnosticError<T>>().map(AsMut::as_mut)
        }
    }

    pub fn downcast<T: 'static>(self) -> Result<Box<T>, Self> {
        let Self {
            inner,
            contexts,
            severity_override,
            session_sources,
            attached_sources,
        } = self;
        if inner.as_any().is::<T>() {
            Ok(inner
                .into_any()
                .downcast()
                .expect("type was checked before consuming owned diagnostic"))
        } else if inner.as_any().is::<DiagnosticError<T>>() {
            let diagnostic_error = inner
                .into_any()
                .downcast::<DiagnosticError<T>>()
                .expect("type was checked before consuming owned diagnostic");
            Ok(Box::new(diagnostic_error.0))
        } else {
            Err(Self {
                inner,
                contexts,
                severity_override,
                session_sources,
                attached_sources,
            })
        }
    }

    fn prepared<'a>(
        &'a self,
        snapshot: crate::DiagnosticSnapshot,
        session_sources: &'a dyn SourceProvider,
    ) -> PreparedDiagnostic<'a> {
        let attached = self.attached_sources().map(|sources| sources as &dyn SourceProvider);
        PreparedDiagnostic {
            snapshot,
            sources: LayeredSourceProvider::new(session_sources, attached),
        }
    }
}

impl fmt::Display for OwnedDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        DiagnosticMessage(self.as_diagnostic()).fmt(formatter)
    }
}

/// Delegates the semantic diagnostic protocol to the owned occurrence.
///
/// Occurrence transport metadata such as context frames and attached source providers is consumed
/// by [`OwnedDiagnostic::prepare`]; it is intentionally not exposed when an owned diagnostic is
/// nested as a plain [`Diagnostic`].
impl Diagnostic for OwnedDiagnostic {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        self.as_diagnostic().message(out)
    }

    fn descriptor(&self) -> Option<&'static DiagnosticDescriptor> {
        self.as_diagnostic().descriptor()
    }

    fn code(&self) -> Option<DiagnosticCodeRef<'_>> {
        self.as_diagnostic().code()
    }

    fn severity(&self) -> Severity {
        OwnedDiagnostic::severity(self)
    }

    fn tags(&self) -> &[DiagnosticTag] {
        self.as_diagnostic().tags()
    }

    fn visit(&self, visitor: &mut dyn crate::VisitDiagnostic) {
        self.as_diagnostic().visit(visitor)
    }

    fn cause(&self) -> Option<&(dyn core::error::Error + 'static)> {
        self.as_diagnostic().cause()
    }

    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        self.as_diagnostic().diagnostic_source()
    }
}

impl fmt::Debug for OwnedDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OwnedDiagnostic")
            .field("diagnostic", &self.as_diagnostic())
            .field("effective_severity", &self.severity())
            .field("severity_override", &self.severity_override)
            .field("contexts", &self.contexts)
            .field("has_session_sources", &self.session_sources.is_some())
            .field("has_attached_sources", &self.attached_sources.is_some())
            .finish()
    }
}

/// A borrowing adapter for rich formatting of one owned diagnostic.
///
/// The adapter uses deterministic [`RenderConfig::DEFAULT`] settings unless
/// configured otherwise. Its [`fmt::Display`] implementation degrades
/// preparation and rendering failures to safe text; use [`Self::try_render`]
/// when those failures must remain observable.
#[derive(Clone, Copy)]
#[must_use = "a diagnostic display adapter must be formatted or rendered"]
pub struct DiagnosticDisplay<'a> {
    diagnostic: &'a OwnedDiagnostic,
    session_sources: &'a dyn SourceProvider,
    config: RenderConfig,
}

impl<'a> DiagnosticDisplay<'a> {
    const fn new(diagnostic: &'a OwnedDiagnostic, session_sources: &'a dyn SourceProvider) -> Self {
        Self {
            diagnostic,
            session_sources,
            config: RenderConfig::DEFAULT,
        }
    }

    /// Replaces the renderer configuration used by this adapter.
    pub const fn with_config(mut self, config: RenderConfig) -> Self {
        self.config = config;
        self
    }

    /// Returns the renderer configuration used by this adapter.
    pub const fn config(&self) -> RenderConfig {
        self.config
    }

    /// Strictly prepares and renders this diagnostic.
    pub fn try_render(&self) -> Result<String, DiagnosticRenderError> {
        let diagnostic = self
            .diagnostic
            .prepare(self.session_sources)
            .map_err(DiagnosticRenderError::Prepare)?;
        AnnotateRenderer::new(self.config)
            .render(&diagnostic)
            .map_err(DiagnosticRenderError::Render)
    }

    fn render_or_degrade(&self) -> String {
        let Ok(diagnostic) = self.diagnostic.prepare(self.session_sources) else {
            return String::from(REPORT_PREPARATION_FALLBACK);
        };
        render_or_degrade(&AnnotateRenderer::new(self.config), &diagnostic).0
    }
}

impl fmt::Display for DiagnosticDisplay<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.render_or_degrade())
    }
}

/// A failed-computation wrapper that promotes its occurrence to error.
pub struct Report {
    inner: Box<OwnedDiagnostic>,
}

impl Report {
    /// Creates a failed-computation report from an ad-hoc display message.
    ///
    /// The message is formatted immediately, so it may borrow local values and does not need to
    /// satisfy the owned diagnostic transport bounds.
    pub fn msg(message: impl fmt::Display) -> Self {
        Self::new(crate::AdHocDiagnostic::new(format_args!("{message}")))
    }

    /// Creates a failed-computation report.
    ///
    /// ```compile_fail
    /// extern crate alloc;
    /// use core::fmt;
    /// use miden_diagnostics::{Diagnostic, Report};
    ///
    /// #[derive(Debug)]
    /// struct Borrowed<'a>(&'a str);
    /// impl Diagnostic for Borrowed<'_> {
    ///     fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
    ///         out.write_str(self.0)
    ///     }
    /// }
    ///
    /// let text = alloc::string::String::from("local");
    /// let _ = Report::new(Borrowed(&text));
    /// ```
    ///
    /// ```compile_fail
    /// extern crate alloc;
    /// use alloc::rc::Rc;
    /// use core::{cell::Cell, fmt};
    /// use miden_diagnostics::{Diagnostic, Report};
    ///
    /// #[derive(Debug)]
    /// struct Local(Rc<Cell<u32>>);
    /// impl Diagnostic for Local {
    ///     fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
    ///         write!(out, "{}", self.0.get())
    ///     }
    /// }
    ///
    /// let _ = Report::new(Local(Rc::new(Cell::new(1))));
    /// ```
    pub fn new<T>(diagnostic: T) -> Self
    where
        T: Diagnostic + Send + Sync + 'static,
    {
        Self {
            inner: Box::new(
                OwnedDiagnostic::new(diagnostic).with_severity_override(Severity::Error),
            ),
        }
    }

    pub fn from_diagnostic(mut diagnostic: OwnedDiagnostic) -> Self {
        diagnostic.set_severity_override(Some(Severity::Error));
        Self {
            inner: Box::new(diagnostic),
        }
    }

    pub fn from_error<E: core::error::Error + Send + Sync + 'static>(error: E) -> Self {
        Self::new(DiagnosticError(error))
    }

    pub fn as_diagnostic(&self) -> &dyn Diagnostic {
        self.inner.as_diagnostic()
    }

    pub fn severity(&self) -> Severity {
        self.inner.severity()
    }

    pub fn descriptor(&self) -> Option<&'static DiagnosticDescriptor> {
        self.inner.descriptor()
    }

    pub fn contexts(&self) -> &[ContextFrame] {
        self.inner.contexts()
    }

    pub fn context(mut self, context: impl Into<String>) -> Self {
        self.inner.push_context(context);
        self
    }

    pub fn with_context<F, S>(self, context: F) -> Self
    where
        F: FnOnce() -> S,
        S: Into<String>,
    {
        self.context(context())
    }

    pub fn attach_sources<P>(mut self, sources: P) -> Self
    where
        P: SourceProvider + Send + Sync + 'static,
    {
        self.inner.attached_sources = Some(Box::new(sources));
        self
    }

    /// Retains a session source provider with this report.
    pub fn attach_session_sources<P>(mut self, sources: P) -> Self
    where
        P: SourceProvider + Send + Sync + 'static,
    {
        self.inner.set_session_sources(sources);
        self
    }

    pub fn session_sources(&self) -> Option<&(dyn SourceProvider + Send + Sync + 'static)> {
        self.inner.session_sources()
    }

    pub fn attached_sources(&self) -> Option<&(dyn SourceProvider + Send + Sync + 'static)> {
        self.inner.attached_sources()
    }

    /// Prepares this report with its occurrence metadata and source universes.
    pub fn prepare<'a>(
        &'a self,
        session_sources: &'a dyn SourceProvider,
    ) -> Result<PreparedDiagnostic<'a>, PrepareError> {
        self.inner.prepare(session_sources)
    }

    /// Prepares this report with explicit resource limits.
    pub fn prepare_with_limits<'a>(
        &'a self,
        session_sources: &'a dyn SourceProvider,
        limits: PreparationLimits,
    ) -> Result<PreparedDiagnostic<'a>, PrepareError> {
        self.inner.prepare_with_limits(session_sources, limits)
    }

    /// Prepares this report using only sources attached to it.
    pub fn prepare_attached(&self) -> Result<PreparedDiagnostic<'_>, PrepareError> {
        self.inner.prepare_attached()
    }

    /// Prepares this report using only attached sources and explicit limits.
    pub fn prepare_attached_with_limits(
        &self,
        limits: PreparationLimits,
    ) -> Result<PreparedDiagnostic<'_>, PrepareError> {
        self.inner.prepare_attached_with_limits(limits)
    }

    /// Returns a rich-formatting adapter using attached sources and portable defaults.
    pub fn display(&self) -> DiagnosticDisplay<'_> {
        self.inner.display()
    }

    /// Returns a rich-formatting adapter with an explicit session source provider.
    pub fn display_with_sources<'a>(
        &'a self,
        session_sources: &'a dyn SourceProvider,
    ) -> DiagnosticDisplay<'a> {
        self.inner.display_with_sources(session_sources)
    }

    pub fn is<T: 'static>(&self) -> bool {
        self.inner.is::<T>()
    }

    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.inner.downcast_ref()
    }

    pub fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.inner.downcast_mut()
    }

    pub fn downcast<T: 'static>(self) -> Result<Box<T>, Self> {
        match (*self.inner).downcast() {
            Ok(value) => Ok(value),
            Err(inner) => Err(Self {
                inner: Box::new(inner),
            }),
        }
    }

    pub fn into_diagnostic(self) -> OwnedDiagnostic {
        *self.inner
    }
}

impl fmt::Display for Report {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        DiagnosticMessage(self.inner.as_diagnostic()).fmt(formatter)?;
        if formatter.alternate() {
            for context in self.inner.contexts().iter().rev() {
                write!(formatter, "\n  context: {}", context.message())?;
            }
            let mut cause = self.inner.as_diagnostic().cause();
            let mut depth = 0_u8;
            while let Some(error) = cause {
                write!(formatter, "\n  caused by: {error}")?;
                cause = error.source();
                depth = depth.saturating_add(1);
                if depth == 64 && cause.is_some() {
                    formatter.write_str("\n  caused by: <cause depth limit reached>")?;
                    break;
                }
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Report {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.display(), formatter)
    }
}

impl core::error::Error for Report {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        self.inner.as_diagnostic().cause()
    }
}

impl From<Report> for OwnedDiagnostic {
    fn from(report: Report) -> Self {
        report.into_diagnostic()
    }
}

impl<T: Diagnostic + Send + Sync + 'static> From<T> for Report {
    fn from(value: T) -> Self {
        Report::new(value)
    }
}

/// Convenience [`Diagnostic`] that can be used as an "anonymous" wrapper for
/// Errors. This is intended to be paired with [`IntoDiagnostic`].
#[derive(Debug)]
#[repr(transparent)]
struct DiagnosticError<T>(T);

impl<T> AsRef<T> for DiagnosticError<T> {
    #[inline(always)]
    fn as_ref(&self) -> &T {
        &self.0
    }
}

impl<T> AsMut<T> for DiagnosticError<T> {
    #[inline(always)]
    fn as_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: fmt::Display> fmt::Display for DiagnosticError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl<T: core::error::Error> core::error::Error for DiagnosticError<T> {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        self.0.source()
    }
}

impl<T: core::error::Error + 'static> Diagnostic for DiagnosticError<T> {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_fmt(format_args!("{}", self.0))
    }

    fn cause(&self) -> Option<&(dyn core::error::Error + 'static)> {
        self.0.source()
    }
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, format, string::ToString};
    use core::fmt::Write;

    use super::*;
    use crate::{
        DescriptorOrigin, DiagnosticCode, DiagnosticDescriptor, DiagnosticTag, Explanation, Label,
        LabelStyle, SourceMap, SourceNamespace, SourceRevision, SourceSpan, TextRange,
        VisitDiagnostic,
    };

    static WARNING: DiagnosticDescriptor = DiagnosticDescriptor {
        code: DiagnosticCode {
            namespace: "test",
            code: "W0001",
        },
        summary: "warning",
        default_severity: Severity::Warning,
        explanation: Explanation::NotProvided,
        documentation_url: None,
        tags: &[DiagnosticTag::Deprecated],
        origin: DescriptorOrigin {
            module_path: module_path!(),
            file: file!(),
            line: line!(),
        },
    };

    #[derive(Debug)]
    struct Mutable(u32);

    impl Diagnostic for Mutable {
        fn message(&self, out: &mut dyn Write) -> fmt::Result {
            write!(out, "value {}", self.0)
        }
    }

    #[test]
    fn hidden_erasure_supports_all_safe_downcast_paths() {
        fn require_transport<T: Send + Sync + 'static>() {}
        require_transport::<OwnedDiagnostic>();
        require_transport::<Report>();

        let mut owned = OwnedDiagnostic::new(Mutable(1));
        assert!(owned.is::<Mutable>());
        assert_eq!(owned.downcast_ref::<Mutable>().unwrap().0, 1);
        owned.downcast_mut::<Mutable>().unwrap().0 = 2;
        assert_eq!(owned.downcast::<Mutable>().unwrap().0, 2);

        let mut sources = SourceMap::new(SourceNamespace::new_unchecked(9));
        let source_id = sources.insert("attached", "text", None).unwrap();
        let failed = OwnedDiagnostic::new(Mutable(3))
            .with_context("kept")
            .attach_sources(sources)
            .downcast::<alloc::string::String>()
            .unwrap_err();
        assert_eq!(failed.contexts()[0].message(), "kept");
        assert_eq!(failed.attached_sources().unwrap().get(source_id).unwrap().text, Some("text"));

        let report = Report::new(Mutable(4))
            .context("report context")
            .downcast::<alloc::string::String>()
            .unwrap_err();
        assert_eq!(report.severity(), Severity::Error);
        assert_eq!(report.contexts()[0].message(), "report context");
        assert_eq!(report.downcast_ref::<Mutable>().unwrap().0, 4);
    }

    #[test]
    fn report_is_a_pointer_sized_failure_handle() {
        assert_eq!(core::mem::size_of::<Report>(), core::mem::size_of::<usize>());
    }

    #[derive(Debug)]
    struct Warning;

    impl Diagnostic for Warning {
        fn message(&self, out: &mut dyn Write) -> fmt::Result {
            out.write_str("warning occurrence")
        }

        fn descriptor(&self) -> Option<&'static DiagnosticDescriptor> {
            Some(&WARNING)
        }
    }

    #[test]
    fn report_promotes_occurrence_without_mutating_descriptor_identity() {
        let owned = OwnedDiagnostic::new(Warning);
        assert_eq!(owned.severity(), Severity::Warning);
        assert!(core::ptr::eq(owned.descriptor().unwrap(), &WARNING));

        let report = Report::new(Warning).context("inner").with_context(|| "outer".to_string());
        assert_eq!(report.severity(), Severity::Error);
        assert!(core::ptr::eq(report.descriptor().unwrap(), &WARNING));
        assert_eq!(WARNING.default_severity, Severity::Warning);
        assert_eq!(report.to_string(), "warning occurrence");
        assert_eq!(format!("{report:#}"), "warning occurrence\n  context: outer\n  context: inner");

        let diagnostic = report.into_diagnostic();
        assert_eq!(diagnostic.severity(), Severity::Error);
        assert!(core::ptr::eq(diagnostic.descriptor().unwrap(), &WARNING));
    }

    #[test]
    fn report_msg_formats_borrowed_values_immediately() {
        let message = String::from("ad-hoc failure");
        let report = Report::msg(&message);
        drop(message);
        assert_eq!(report.to_string(), "ad-hoc failure");
    }

    #[derive(Debug)]
    struct LabeledFailure(SourceSpan);

    impl Diagnostic for LabeledFailure {
        fn message(&self, out: &mut dyn Write) -> fmt::Result {
            out.write_str("labeled failure")
        }

        fn visit(&self, visitor: &mut dyn VisitDiagnostic) {
            visitor.label(Label {
                span: self.0,
                style: LabelStyle::Primary,
                message: Some(format_args!("failure occurred here")),
            });
        }
    }

    #[test]
    fn report_display_uses_retained_session_sources() {
        let mut sources = SourceMap::new(SourceNamespace::new_unchecked(12));
        let id = sources.insert("input.masm", "begin broken end", None).unwrap();
        let span = SourceSpan::session(id, TextRange::new(6, 12).unwrap());
        let report = Report::new(LabeledFailure(span)).attach_session_sources(sources);

        let rendered = format!("{report:?}");
        assert!(rendered.contains("input.masm"));
        assert!(rendered.contains("broken"));
        assert!(rendered.contains("failure occurred here"));
    }

    #[test]
    fn unknown_labels_do_not_require_a_source_provider() {
        let report = Report::new(LabeledFailure(SourceSpan::UNKNOWN));
        let rendered = format!("{report:?}");
        assert!(rendered.contains("labeled failure"));
        assert!(!rendered.contains("missing source"));
    }

    #[derive(Debug)]
    struct Cause;

    impl fmt::Display for Cause {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("root cause")
        }
    }

    impl core::error::Error for Cause {}

    #[derive(Debug)]
    struct WithCause(Cause);

    impl Diagnostic for WithCause {
        fn message(&self, out: &mut dyn Write) -> fmt::Result {
            out.write_str("operation failed")
        }

        fn cause(&self) -> Option<&(dyn core::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn report_delegates_error_source_and_retains_attached_bundle() {
        let mut sources = SourceMap::new(SourceNamespace::new_unchecked(4));
        let id = sources.insert("input", "source", Some(SourceRevision(1))).unwrap();
        let report = Report::new(WithCause(Cause)).attach_sources(sources);
        assert_eq!(core::error::Error::source(&report).unwrap().to_string(), "root cause");
        assert_eq!(format!("{report:#}"), "operation failed\n  caused by: root cause");
        let diagnostic = report.into_diagnostic();
        let source = diagnostic.attached_sources().unwrap().get(id).unwrap();
        assert_eq!(source.text, Some("source"));
        assert_eq!(source.revision, Some(SourceRevision(1)));
    }

    #[derive(Debug)]
    struct ChainCause(Option<Box<ChainCause>>);

    impl fmt::Display for ChainCause {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("chain")
        }
    }

    impl core::error::Error for ChainCause {
        fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
            self.0.as_deref().map(|cause| cause as &(dyn core::error::Error + 'static))
        }
    }

    #[derive(Debug)]
    struct WithChain(ChainCause);

    impl Diagnostic for WithChain {
        fn message(&self, out: &mut dyn Write) -> fmt::Result {
            out.write_str("with chain")
        }

        fn cause(&self) -> Option<&(dyn core::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    fn cause_chain(length: usize) -> ChainCause {
        let mut cause = None;
        for _ in 0..length {
            cause = Some(Box::new(ChainCause(cause)));
        }
        *cause.expect("test cause chain must be nonempty")
    }

    #[test]
    fn alternate_display_marks_only_a_genuinely_truncated_cause_chain() {
        let exact_limit = format!("{:#}", Report::new(WithChain(cause_chain(64))));
        assert_eq!(exact_limit.matches("\n  caused by: chain").count(), 64);
        assert!(!exact_limit.contains("<cause depth limit reached>"));

        let over_limit = format!("{:#}", Report::new(WithChain(cause_chain(65))));
        assert_eq!(over_limit.matches("\n  caused by: chain").count(), 64);
        assert!(over_limit.ends_with("\n  caused by: <cause depth limit reached>"));
    }

    #[test]
    fn equal_attached_source_ids_remain_scoped_to_their_own_occurrence() {
        let namespace = SourceNamespace::new_unchecked(12);
        let mut first_sources = SourceMap::new(namespace);
        let mut second_sources = SourceMap::new(namespace);
        let first_id = first_sources.insert("same", "first", None).unwrap();
        let second_id = second_sources.insert("same", "second", None).unwrap();
        assert_eq!(first_id, second_id);

        let first = OwnedDiagnostic::new(Mutable(1)).attach_sources(first_sources);
        let second = OwnedDiagnostic::new(Mutable(2)).attach_sources(second_sources);

        assert_eq!(first.attached_sources().unwrap().get(first_id).unwrap().text, Some("first"));
        assert_eq!(second.attached_sources().unwrap().get(second_id).unwrap().text, Some("second"));
    }
}
