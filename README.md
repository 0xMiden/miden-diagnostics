# miden-diagnostics

This crate provides useful infrastructure for compiler diagnostics, which are intended to
be shared/reused across various Miden components that perform some type of compilation, e.g.
AirScript, the (in development) Miden IR, and Miden Assembly.

See [miden-parsing](https://github.com/0xPolygonMiden/miden-parsing) for parsing utilities that
build on top of the low-level tools provided here. A complete compiler frontend is expected to
build a lexer on top of that crate's `Scanner` type, and implement it's `Parser` trait. Many
of the details involved in producing an AST with source spans are handled by that crate, but it
is entirely possible to forge your own alternative.

## Components

This crate provides functionality for two distinct, but inter-related use cases:

* Tracking compiler sources, locations/spans within those sources, and supporting infra
for associating spans to Rust data structures, e.g. `#[derive(Spanned)]`
* Constructing, emitting, and displaying/capturing compiler diagnostics, optionally decorated
with source spans for rendering `rustc`-like messages/warnings/errors.

### Source-Level Debugging Info

We build upon some of the primitives provided by the [codespan](https://crates.io/crates/codespan) 
crate to provide a rich set of functionality around tracking sources, locations, and spans in as
efficient a way as possible. The intent is to ensure that decorating compiler structures with source
locations is as cheap as possible, while preserving the ability to easily obtain useful information
about those structures, such as what file/line/column a given object was derived from.

The following are the key features that support this use case:

* `SourceId` is a compact reference to a specific source file that was loaded into memory
* `SourceIndex` is a compact reference to a specific location in some source file
* `SourceSpan` is a compact structure which refers to a specific range of locations in some source file.
This type is the most common value type you will interact with, and is used to generate pretty diagnostics 
that point to a specific range of characters in a source file to which the diagnostic pertains.
* `Span<T>`, is a type used to associate a `SourceSpan` with a type `T` non-invasively; derefs to `T`,
and implements a variety of other traits that delegate to `T` in a pass-through fashion, e.g. `PartialEq`
* `Spanned` is a trait which types may implement to produce a `SourceSpan` upon request. The `Span<T>` type
implements this, and it is automatically implemented for all `Box<T>` where `T: Spanned`.
* The `CodeMap` is a thread-safe datastructure that is intended to be constructed once by a compiler driver
and shared across all its child threads. It stores files read into memory, de-duplicating by the name of the
source file (whether real or synthetic). It provides APIs which can be used to obtain useful high-level information
from a `SourceId`, `SourceIndex`, or `SourceSpan`, such as the file name, line and column numbers;
as well as obtain a slice of the original source content.

### Diagnostics

We build upon some utilities provided by the [codespan_reporting](https://crates.io/crates/codespan-reporting) 
crate to provide a richer set of features for generating, displaying and/or capturing compiler diagnostics.

The following are the key features that support this use case:

* `Diagnostic` is a type that represents a compiler diagnostic, with a severity, a message, with an 
(optional) set of labels/notes.
* `ToDiagnostic` is a trait which represents the ability to generate a `Diagnostic` from a type. In
practice this is used with errors which are converted to diagnostics when compilation should proceed
because an error is non-fatal. For example, during parsing/semantic analysis you typically want to 
capture as many errors as possible before failing the compilation task, rather than exiting on the
first error encountered.
* `Severity` represents whether a diagnostic is an error, warning, bug, or simple note.
* `Label` is used to associate a source location with a diagnostic, with some descriptive text. Labels
come in primary/secondary flavors, which affect how the labels are ordered/rendered when displayed.
* `InFlightDiagnostic` provides a fluent, builder-pattern API for constructing and emitting diagnostics
on the fly, see examples below.
* `Emitter` is a trait that can be used to control how diagnostics are emitted. This can be used to do useful
things such as disable diagnostics, capture them for tests, or control how they are displayed; the
built-in `NullEmitter`, `CaptureEmitter`, and `DefaultEmitter` types perform those respective functions.
* `DiagnosticsHandler` is a thread-safe type meant to be constructed once by the compiler driver and then
shared across all threads of execution. It can be configured to use a particular `Emitter` implementation,
control what the minimum severity of emitted diagnostics are, convert warnings to errors, and more.

### Examples

#### Abstract Syntax Tree

```rust
use miden_diagnostics::{SourceSpan, Span, Spanned};

#[derive(Clone, PartialEq, Eq, Hash, Spanned)]
pub struct Ident(#[span] Span<String>);

#[derive(Spanned)]
pub enum Expr {
    Var(#[span] Ident),
    Int(#[span] Span<i64>),
    Let(#[span] Let),
    Binary(#[span] BinaryExpr),
    Unary(#[span] UnaryExpr),
}

#[derive(Spanned)]
pub struct Let {
    pub span: SourceSpan,
    pub var: Ident,
    pub body: Box<Expr>,
}

#[derive(Spanned)]
pub struct BinaryExpr {
    pub span: SourceSpan,
    pub op: BinaryOp,
    pub lhs: Box<Expr>,
    pub rhs: Box<Expr>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derived(Spanned)]
pub struct UnaryExpr {
    pub span: SourceSpan,
    pub op: UnaryOp,
    pub rhs: Box<Expr>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
}
```

#### Diagnostics

```rust
use std::sync::Arc;

use miden_diagnostics::*;

const INPUT_FILE = r#"
let x = 42
in
  let y = x * 2
  in
    x + y
"#;

pub fn main() -> Result<(), ()> {
    // The codemap is where parsed inputs are stored and is the base for all source locations
    let codemap = Arc::new(CodeMap::new());
    // The emitter defines how diagnostics will be emitted/displayed
    let emitter = Arc::new(DefaultEmitter::new(ColorChoice::Auto));
    // The config provides some control over what diagnostics to display and how
    let config = DiagnosticsConfig::default();
    // The diagnostics handler itself is used to emit diagnostics
    let diagnostics = Arc::new(DiagnosticsHandler::new(config, codemap.clone(), emitter));

    // In our example, we're adding an input to the codemap, and then requesting the compiler compile it
    codemap.add("nofile", INPUT_FILE.to_string());
    compiler::compile(codemap, diagnostics, "nofile")
}

mod compiler {
    use miden_diagnostics::*;

    pub fn compile<F: Into<FileName>>(codemap: Arc<CodeMap>, diagnostics: Arc<DiagnosticsHandler>, filename: F) -> Result<(), ()> {
        let filename = filename.into();
        let file = codemap.get_by_name(&filename).unwrap();

        // The details of parsing are left as an exercise for the reader, but it is expected
        // that for Miden projects that this crate will be combined with `miden-parsing` to
        // handle many of the details involved in producing a stream of tokens from raw sources
        //
        // In this case, we're parsing an Expr, or returning an error that has an associated source span
        match parser::parse(file.source(), &diagnostics)? {
            Ok(_expr) => Ok(()),
            Err(err) => {
                diagnostics.diagnostic(Severity::Error)
                  .with_message("parsing failed")
                  .with_primary_label(err.span(), err.to_string())
                  .emit();
                Err(())
            }
        }
    }
}
```
