#![no_std]
#![forbid(unsafe_code)]

pub use diag_runtime::Diagnostic;

#[derive(Debug, Diagnostic)]
#[diagnostic(
    code = "derive-fixture/embedded",
    summary = "feature-unified explanation",
    explanation = include_str!("../explanations/inline.md"),
    message = "embedded definition"
)]
pub struct EmbeddedDefinition;

#[derive(Debug, Diagnostic)]
#[diagnostic(
    code = "derive-fixture/not-provided",
    summary = "no explanation was authored",
    message = "not provided"
)]
pub struct NotProvidedDefinition;

static DESCRIPTOR: &str = "consumer item";
struct RegistryEntry;
fn descriptor() {}

#[derive(Debug, Diagnostic)]
#[diagnostic(
    code = "derive-fixture/collision-one",
    summary = "first collision check",
    message = "one"
)]
pub struct CollisionOne;

#[derive(Debug, Diagnostic)]
#[diagnostic(
    code = "derive-fixture/collision-two",
    summary = "second collision check",
    message = "two"
)]
pub struct CollisionTwo;

#[derive(Debug, Diagnostic)]
#[diagnostic(
    code = "derive-fixture/generic",
    summary = "generic derive",
    message = "generic {value}"
)]
pub struct Generic<T: core::fmt::Display + core::fmt::Debug> {
    pub value: T,
}

pub mod explicit_runtime {
    pub use diag_runtime::{
        __include_explanation, __register_descriptor, Applicability, DescriptorOrigin, Diagnostic,
        DiagnosticCode, DiagnosticCodeRef, DiagnosticDescriptor, DiagnosticTag, Explanation, Label,
        LabelStyle, Note, NoteKind, Severity, Suggestion, TextEdit, VisitDiagnostic,
    };
}

#[derive(Debug, Diagnostic)]
#[diagnostic(
    crate = crate::explicit_runtime,
    code = "derive-fixture/explicit",
    summary = "explicit runtime override",
    message = "explicit"
)]
pub struct ExplicitOverride;

pub fn embedded_state() -> diag_runtime::Explanation {
    use diag_runtime::Diagnostic as _;
    EmbeddedDefinition
        .descriptor()
        .expect("derived descriptor")
        .explanation
}

pub fn not_provided_state() -> diag_runtime::Explanation {
    use diag_runtime::Diagnostic as _;
    NotProvidedDefinition
        .descriptor()
        .expect("derived descriptor")
        .explanation
}

pub fn collision_sentinel() -> &'static str {
    let _ = core::mem::size_of::<RegistryEntry>();
    descriptor();
    DESCRIPTOR
}
