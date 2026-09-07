#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

#[cfg(any(test, feature = "std"))]
extern crate std;

#[cfg(feature = "derive")]
pub use miden_diagnostics_macros::{Diagnostic, Spanned};

mod adhoc;
#[cfg(feature = "std")]
mod application;
mod collection;
mod descriptor;
mod diagnostic;
mod emit;
mod owned;
mod panic_support;
mod registry;
mod render;
mod shims;
mod snapshot;
mod source;
#[cfg(feature = "std")]
mod terminal;

#[doc(hidden)]
pub use adhoc::AdHocDiagnostic;
#[cfg(feature = "std")]
pub use application::ExitWithOutcome;
pub use collection::{
    DefaultFailurePolicy, DiagnosticCollector, DiagnosticEntry, DiagnosticInstanceId,
    DiagnosticLimits, DiagnosticSet, DiagnosticSink, FailurePolicy, MergeSummary, Outcome,
    PushResult, SeverityCounts, WarningsAsErrors,
};
pub use descriptor::{
    DescriptorOrigin, DiagnosticCode, DiagnosticCodeRef, DiagnosticDescriptor, DiagnosticMetadata,
    DiagnosticTag, Explanation, Severity,
};
pub use diagnostic::{
    Applicability, Diagnostic, Label, LabelStyle, Note, NoteKind, Suggestion, TextEdit,
    VisitDiagnostic, VisitTextEdit, VisitTextEdits,
};
pub use emit::{EmissionFailure, EmissionStatus, EmissionSummary, Emitter, FmtEmitter};
#[cfg(feature = "std")]
pub use emit::{IoEmissionError, IoEmitter};
pub use owned::{ContextFrame, DiagnosticDisplay, DiagnosticRenderError, OwnedDiagnostic, Report};
#[cfg(feature = "std")]
pub use panic_support::{InstallHookError, PanicHookOptions, install_panic_hook};
pub use registry::{LookupError, RegistryError, StaticRegistry};
#[cfg(feature = "linked-registry")]
pub use registry::{RegistryIndex, linked_registry};
pub use render::{AnnotateRenderer, RenderConfig, RenderError};
pub use shims::{IntoDiagnostic, WrapErr};
pub use snapshot::{
    DiagnosticCodeOwned, DiagnosticRelation, DiagnosticSnapshot, OwnedCause, OwnedLabel, OwnedNote,
    OwnedSuggestion, OwnedTextEdit, PreparationItemKind, PreparationLimits, PrepareError,
    PreparedDiagnostic, PreparedSet, prepare_ref, prepare_ref_with_limits,
};
pub use source::{
    ColumnIndex, ColumnNumber, LayeredSourceProvider, LineColumn, LineIndex, LineNumber,
    ResolvedSource, SharedSourceProvider, Source, SourceId, SourceKey, SourceMap, SourceMapError,
    SourceNamespace, SourceProvider, SourceRevision, SourceSpan, Span, Spanned, TextRange,
    TextRangeError,
};
#[cfg(feature = "std")]
pub use terminal::{StderrEmitter, StdoutEmitter, TerminalChoice, TerminalPolicy, TerminalWidth};

pub type Result<T, E = Report> = core::result::Result<T, E>;

#[doc(hidden)]
pub mod __private {
    pub use alloc::vec::Vec;

    #[cfg(feature = "linked-registry")]
    pub use inventory;

    #[cfg(feature = "linked-registry")]
    pub use crate::registry::RegistryEntry;
    pub use crate::{
        AdHocDiagnostic, OwnedTextEdit,
        adhoc::{format_arguments, validate_canonical_code},
        panic_support::panic_report,
    };
}

#[macro_export]
macro_rules! panic_report {
    ($report:expr $(,)?) => {{ $crate::__private::panic_report($report) }};
}

#[cfg(feature = "embed-explanations")]
#[doc(hidden)]
#[macro_export]
macro_rules! __include_explanation {
    ($explanation:expr) => {
        $crate::Explanation::Embedded($explanation)
    };
}

#[cfg(not(feature = "embed-explanations"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __include_explanation {
    ($($discarded:tt)*) => {
        $crate::Explanation::NotEmbedded
    };
}

#[cfg(feature = "linked-registry")]
#[doc(hidden)]
#[macro_export]
macro_rules! __register_descriptor {
    ($descriptor:expr) => {
        $crate::__private::inventory::submit! {
            $crate::__private::RegistryEntry::new($descriptor)
        }
    };
}

#[cfg(not(feature = "linked-registry"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __register_descriptor {
    ($($discarded:tt)*) => {
        const _: () = ();
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __diagnostic_severity {
    (Error) => {
        $crate::Severity::Error
    };
    (Warning) => {
        $crate::Severity::Warning
    };
    (Info) => {
        $crate::Severity::Info
    };
    (Hint) => {
        $crate::Severity::Hint
    };
    ($other:ident) => {
        compile_error!(concat!("unsupported diagnostic severity `", stringify!($other), "`"))
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __diagnostic_tag {
    (Unnecessary) => {
        $crate::DiagnosticTag::Unnecessary
    };
    (Deprecated) => {
        $crate::DiagnosticTag::Deprecated
    };
    ($other:ident) => {
        compile_error!(concat!("unsupported diagnostic tag `", stringify!($other), "`"))
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __diagnostic_applicability {
    (MachineApplicable) => {
        $crate::Applicability::MachineApplicable
    };
    (MaybeIncorrect) => {
        $crate::Applicability::MaybeIncorrect
    };
    (HasPlaceholders) => {
        $crate::Applicability::HasPlaceholders
    };
    (Unspecified) => {
        $crate::Applicability::Unspecified
    };
    ($other:ident) => {
        compile_error!(concat!("unsupported diagnostic applicability `", stringify!($other), "`"))
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __diagnostic_explanation {
    () => {
        $crate::Explanation::NotProvided
    };
    ($explanation:expr) => {
        $crate::__include_explanation!($explanation)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __diagnostic_url {
    () => {
        None
    };
    ($url:literal) => {
        Some($url)
    };
}

#[macro_export]
macro_rules! diagnostic_codes {
    (
        namespace = $namespace:literal;
        catalog = $catalog:ident;
        $(
            $visibility:vis $name:ident {
                summary: $summary:literal,
                severity: $severity:ident,
                $(explanation: $explanation:expr,)?
                $(documentation_url: $documentation_url:literal,)?
                $(tags: [$($tag:ident),* $(,)?],)?
            }
        )*
    ) => {
        $(
            const _: () = $crate::__private::validate_canonical_code(
                concat!($namespace, "/", stringify!($name)),
            );
            $visibility static $name: $crate::DiagnosticDescriptor =
                $crate::DiagnosticDescriptor {
                    code: $crate::DiagnosticCode {
                        namespace: $namespace,
                        code: stringify!($name),
                    },
                    summary: $summary,
                    default_severity: $crate::__diagnostic_severity!($severity),
                    explanation:
                        $crate::__diagnostic_explanation!($($explanation)?),
                    documentation_url:
                        $crate::__diagnostic_url!($($documentation_url)?),
                    tags: &[$($($crate::__diagnostic_tag!($tag)),*)?],
                    origin: $crate::DescriptorOrigin {
                        module_path: module_path!(),
                        file: file!(),
                        line: line!(),
                    },
                };
            $crate::__register_descriptor!(&$name);
        )*

        pub static $catalog: &[&'static $crate::DiagnosticDescriptor] =
            &[$(&$name),*];
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __diagnostic_format_args {
    ($message:literal) => {
        format_args!($message)
    };
    ($message:expr) => {
        format_args!("{}", $message)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __diagnostic_push_label {
    ($diagnostic:ident, primary, $span:expr) => {
        $diagnostic.push_label($span, $crate::LabelStyle::Primary, None)
    };
    ($diagnostic:ident, primary, $span:expr, $message:tt) => {
        $diagnostic.push_label(
            $span,
            $crate::LabelStyle::Primary,
            Some($crate::__diagnostic_format_args!($message)),
        )
    };
    ($diagnostic:ident, context, $span:expr) => {
        $diagnostic.push_label($span, $crate::LabelStyle::Context, None)
    };
    ($diagnostic:ident, context, $span:expr, $message:tt) => {
        $diagnostic.push_label(
            $span,
            $crate::LabelStyle::Context,
            Some($crate::__diagnostic_format_args!($message)),
        )
    };
    ($diagnostic:ident, $other:ident, $($rest:tt)*) => {
        compile_error!(concat!("unsupported diagnostic label `", stringify!($other), "`"))
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __diagnostic_push_note {
    ($diagnostic:ident, note, $message:tt) => {
        $diagnostic.push_note($crate::NoteKind::Note, $crate::__diagnostic_format_args!($message))
    };
    ($diagnostic:ident, help, $message:tt) => {
        $diagnostic.push_note($crate::NoteKind::Help, $crate::__diagnostic_format_args!($message))
    };
    ($diagnostic:ident, $other:ident, $($rest:tt)*) => {
        compile_error!(concat!("unsupported diagnostic note `", stringify!($other), "`"))
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __diagnostic_push_suggestions {
    ($diagnostic:ident;) => {};
    (
        $diagnostic:ident;
        suggestion(
            $message:tt,
            applicability: $applicability:ident,
            edits: [
                $(edit($span:expr, $replacement:tt)),+ $(,)?
            ]
        )
        $(, $(
            suggestion(
                $rest_message:tt,
                applicability: $rest_applicability:ident,
                edits: [
                    $(edit($rest_span:expr, $rest_replacement:tt)),+ $(,)?
                ]
            )
        ),* $(,)?)?
    ) => {
        {
            let mut __miden_diagnostic_edits = $crate::__private::Vec::new();
            $(
                __miden_diagnostic_edits.push($crate::OwnedTextEdit {
                    span: $span,
                    replacement: $crate::__private::format_arguments(
                        $crate::__diagnostic_format_args!($replacement),
                    ),
                });
            )+
            $diagnostic.push_suggestion(
                $crate::__diagnostic_format_args!($message),
                $crate::__diagnostic_applicability!($applicability),
                __miden_diagnostic_edits,
            );
        }
        $(
            $(
                {
                    let mut __miden_diagnostic_edits = $crate::__private::Vec::new();
                    $(
                        __miden_diagnostic_edits.push($crate::OwnedTextEdit {
                            span: $rest_span,
                            replacement: $crate::__private::format_arguments(
                                $crate::__diagnostic_format_args!($rest_replacement),
                            ),
                        });
                    )+
                    $diagnostic.push_suggestion(
                        $crate::__diagnostic_format_args!($rest_message),
                        $crate::__diagnostic_applicability!($rest_applicability),
                        __miden_diagnostic_edits,
                    );
                }
            )*
        )?
    };
    ($diagnostic:ident; $($invalid:tt)+) => {
        compile_error!("invalid suggestion syntax; expected `suggestion(message, applicability: Applicability, edits: [edit(span, replacement), ...])`")
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __diagnostic_build {
    (
        $root:expr;
        $(labels: [
            $($label_kind:ident($label_span:expr $(, $label_message:expr)?)),* $(,)?
        ],)?
        $(notes: [
            $($note_kind:ident($note_message:expr)),* $(,)?
        ],)?
        $(suggestions: [
            $($suggestion:tt)*
        ],)?
        $(cause: $cause:expr,)?
        $(diagnostic_source: $diagnostic_source:expr,)?
        $(related: [$($related:expr),* $(,)?],)?
    ) => {
        {
            let mut __miden_diagnostic = $root;
            $(
                $(
                    $crate::__diagnostic_push_label!(
                        __miden_diagnostic,
                        $label_kind,
                        $label_span
                        $(, $label_message)?
                    );
                )*
            )?
            $(
                $(
                    $crate::__diagnostic_push_note!(
                        __miden_diagnostic,
                        $note_kind,
                        $note_message
                    );
                )*
            )?
            $(
                $crate::__diagnostic_push_suggestions!(
                    __miden_diagnostic;
                    $($suggestion)*
                );
            )?
            $(
                __miden_diagnostic.set_cause($cause);
            )?
            $(
                __miden_diagnostic.set_diagnostic_source($diagnostic_source);
            )?
            $(
                $(
                    __miden_diagnostic.push_related($related);
                )*
            )?
            __miden_diagnostic
        }
    };
}

#[macro_export]
macro_rules! diagnostic {
    (
        descriptor: $descriptor:expr,
        severity: $severity:ident,
        message: $message:tt
        $(, $($fields:tt)*)?
    ) => {
        $crate::__diagnostic_build!(
            $crate::__private::AdHocDiagnostic::new(
                $crate::__diagnostic_format_args!($message),
            )
            .with_descriptor($descriptor)
            .with_severity($crate::__diagnostic_severity!($severity));
            $($($fields)*)?
        )
    };
    (
        descriptor: $descriptor:expr,
        message: $message:tt
        $(, $($fields:tt)*)?
    ) => {
        $crate::__diagnostic_build!(
            $crate::__private::AdHocDiagnostic::new(
                $crate::__diagnostic_format_args!($message),
            )
            .with_descriptor($descriptor);
            $($($fields)*)?
        )
    };
    (
        severity: $severity:ident,
        code: $code:literal,
        message: $message:tt
        $(, $($fields:tt)*)?
    ) => {
        {
            const _: () = $crate::__private::validate_canonical_code($code);
            $crate::__diagnostic_build!(
                $crate::__private::AdHocDiagnostic::new(
                    $crate::__diagnostic_format_args!($message),
                )
                .with_canonical_code($code)
                .with_severity($crate::__diagnostic_severity!($severity));
                $($($fields)*)?
            )
        }
    };
    (
        severity: $severity:ident,
        message: $message:tt
        $(, $($fields:tt)*)?
    ) => {
        $crate::__diagnostic_build!(
            $crate::__private::AdHocDiagnostic::new(
                $crate::__diagnostic_format_args!($message),
            )
            .with_severity($crate::__diagnostic_severity!($severity));
            $($($fields)*)?
        )
    };
    (
        code: $code:literal,
        message: $message:tt
        $(, $($fields:tt)*)?
    ) => {
        $crate::diagnostic!(
            severity: Error,
            code: $code,
            message: $message
            $(, $($fields)*)?
        )
    };
    (
        message: $message:tt
        $(, $($fields:tt)*)?
    ) => {
        $crate::diagnostic!(
            severity: Error,
            message: $message
            $(, $($fields)*)?
        )
    };
}

#[macro_export]
macro_rules! report {
    (severity: Warning, $($rest:tt)*) => {
        compile_error!("report! cannot construct a literal Warning diagnostic; use diagnostic!")
    };
    (severity: Info, $($rest:tt)*) => {
        compile_error!("report! cannot construct a literal Info diagnostic; use diagnostic!")
    };
    (severity: Hint, $($rest:tt)*) => {
        compile_error!("report! cannot construct a literal Hint diagnostic; use diagnostic!")
    };
    (descriptor: $descriptor:expr, severity: Warning, $($rest:tt)*) => {
        compile_error!("report! cannot construct a literal Warning diagnostic; use diagnostic!")
    };
    (descriptor: $descriptor:expr, severity: Info, $($rest:tt)*) => {
        compile_error!("report! cannot construct a literal Info diagnostic; use diagnostic!")
    };
    (descriptor: $descriptor:expr, severity: Hint, $($rest:tt)*) => {
        compile_error!("report! cannot construct a literal Hint diagnostic; use diagnostic!")
    };
    (severity: Error, $($rest:tt)*) => {
        $crate::Report::new($crate::diagnostic!(severity: Error, $($rest)*))
    };
    (descriptor: $descriptor:expr, severity: Error, $($rest:tt)*) => {
        $crate::Report::new($crate::diagnostic!(
            descriptor: $descriptor,
            severity: Error,
            $($rest)*
        ))
    };
    (descriptor: $descriptor:expr, $($rest:tt)*) => {
        $crate::Report::new($crate::diagnostic!(
            descriptor: $descriptor,
            $($rest)*
        ))
    };
    (code: $code:literal, $($rest:tt)*) => {
        $crate::Report::new($crate::diagnostic!(
            severity: Error,
            code: $code,
            $($rest)*
        ))
    };
    (message: $message:tt $(, $($rest:tt)*)?) => {
        $crate::Report::new($crate::diagnostic!(
            severity: Error,
            message: $message
            $(, $($rest)*)?
        ))
    };
    ($diagnostic:expr $(,)?) => {
        $crate::Report::new($diagnostic)
    };
}
