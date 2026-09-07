#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;
use renamed_diagnostics::{Diagnostic as _, OwnedLabel, SourceSpan};

static DESCRIPTOR: &str = "consumer item";
struct RegistryEntry;
fn descriptor() {}

#[derive(Debug, renamed_diagnostics::Diagnostic)]
#[diagnostic(
    code = "derive-ui/one",
    summary = "first inline derive",
    message = "one"
)]
pub struct One;

#[derive(Debug, renamed_diagnostics::Diagnostic)]
#[diagnostic(
    code = "derive-ui/two",
    summary = "second inline derive",
    message = "two"
)]
pub struct Two;

#[derive(Debug, renamed_diagnostics::Diagnostic)]
#[diagnostic(message = "named {value} {out} {visitor}")]
pub struct Named<T: core::fmt::Display + core::fmt::Debug> {
    pub value: T,
    pub out: u8,
    pub visitor: u8,
    #[label]
    pub at: Option<SourceSpan>,
    #[labels]
    pub labels: Vec<OwnedLabel>,
}

#[derive(Debug, renamed_diagnostics::Diagnostic)]
#[diagnostic(message = "tuple")]
pub struct Tuple(pub SourceSpan);

#[derive(Debug, renamed_diagnostics::Diagnostic)]
#[diagnostic(message = "unit")]
pub struct Unit;

pub mod explicit_runtime {
    pub use renamed_diagnostics::{
        __include_explanation, __register_descriptor, Applicability, DescriptorOrigin, Diagnostic,
        DiagnosticCode, DiagnosticCodeRef, DiagnosticDescriptor, DiagnosticTag, Explanation, Label,
        LabelStyle, Note, NoteKind, Severity, Suggestion, TextEdit, VisitDiagnostic,
    };
}

#[derive(Debug, renamed_diagnostics::Diagnostic)]
#[diagnostic(crate = crate::explicit_runtime, message = "explicit")]
pub struct Explicit;

pub fn exercise() -> usize {
    assert_eq!(One.descriptor().unwrap().code.code, "one");
    assert_eq!(Two.descriptor().unwrap().code.code, "two");
    assert_eq!(DESCRIPTOR, "consumer item");
    let _ = core::mem::size_of::<RegistryEntry>();
    descriptor();
    2
}
