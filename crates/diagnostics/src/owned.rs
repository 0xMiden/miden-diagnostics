use alloc::{boxed::Box, string::String, vec::Vec};
use core::{any::Any, fmt};

use crate::{
    AnnotateRenderer, Diagnostic, DiagnosticCodeRef, DiagnosticDescriptor, DiagnosticMetadata,
    DiagnosticTag, LayeredSourceProvider, PreparedDiagnostic, RenderConfig, Severity,
    SourceProvider, diagnostic::DiagnosticMessage, emit::render_or_degrade,
    snapshot::prepare_owned_ref, source::EmptySourceProvider,
};

const REPORT_PREPARATION_FALLBACK: &str =
    "error: diagnostic preparation failed; rich output unavailable";

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

    pub fn attached_sources(&self) -> Option<&(dyn SourceProvider + Send + Sync + 'static)> {
        self.attached_sources.as_deref()
    }

    pub fn is<T: 'static>(&self) -> bool {
        self.inner.as_any().is::<T>()
    }

    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.inner.as_any().downcast_ref()
    }

    pub fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.inner.as_any_mut().downcast_mut()
    }

    pub fn downcast<T: 'static>(self) -> Result<Box<T>, Self> {
        let Self {
            inner,
            contexts,
            severity_override,
            attached_sources,
        } = self;
        if inner.as_any().is::<T>() {
            Ok(inner
                .into_any()
                .downcast()
                .expect("type was checked before consuming owned diagnostic"))
        } else {
            Err(Self {
                inner,
                contexts,
                severity_override,
                attached_sources,
            })
        }
    }
}

impl fmt::Display for OwnedDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        DiagnosticMessage(self.as_diagnostic()).fmt(formatter)
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
            .field("has_attached_sources", &self.attached_sources.is_some())
            .finish()
    }
}

/// A failed-computation wrapper that promotes its occurrence to error.
pub struct Report {
    inner: OwnedDiagnostic,
}

impl Report {
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
            inner: OwnedDiagnostic::new(diagnostic).with_severity_override(Severity::Error),
        }
    }

    pub fn from_diagnostic(mut diagnostic: OwnedDiagnostic) -> Self {
        diagnostic.set_severity_override(Some(Severity::Error));
        Self { inner: diagnostic }
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
        self.inner = self.inner.attach_sources(sources);
        self
    }

    pub fn attached_sources(&self) -> Option<&(dyn SourceProvider + Send + Sync + 'static)> {
        self.inner.attached_sources()
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
        match self.inner.downcast() {
            Ok(value) => Ok(value),
            Err(inner) => Err(Self { inner }),
        }
    }

    pub fn into_diagnostic(self) -> OwnedDiagnostic {
        self.inner
    }

    pub(crate) fn render_record(&self, config: RenderConfig) -> String {
        let Ok(snapshot) = prepare_owned_ref(&self.inner) else {
            return String::from(REPORT_PREPARATION_FALLBACK);
        };
        let session = EmptySourceProvider;
        let attached = self.inner.attached_sources().map(|sources| sources as &dyn SourceProvider);
        let diagnostic = PreparedDiagnostic {
            snapshot,
            sources: LayeredSourceProvider::new(&session, attached),
        };
        render_or_degrade(&AnnotateRenderer::new(config), &diagnostic).0
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
        formatter.write_str(&self.render_record(RenderConfig::DEFAULT))
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

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, format, string::ToString};
    use core::fmt::Write;

    use super::*;
    use crate::{
        DescriptorOrigin, DiagnosticCode, DiagnosticDescriptor, DiagnosticTag, Explanation,
        SourceMap, SourceNamespace, SourceRevision,
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

        let mut sources = SourceMap::new(SourceNamespace(9));
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
        let mut sources = SourceMap::new(SourceNamespace(4));
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
        let namespace = SourceNamespace(12);
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
