# miden-diagnostics

This crate provides structured diagnostic infra for Rust libraries, compilers, and command-line applications. It is inspired in part by `miette` and previous attempts at unifying diagnostic infra across the Miden toolchain.

This crate separates four different diagnostic-related concerns:

- Representing individual diagnostics and their severity (see `Diagnostic`)
- Separating the concept of fallibility from diagnostics themselves (see `Outcome<T>` and the `DiagnosticCollector`)
- Representing fallible APIs as `Result`-returning APIs which produce an error diagnostic on failure (see `Report`).
- Application-level concerns from library concerns; preparation, rendering, emission, and failure policy are application-level decisions - while libraries define their diagnostics, and choose how those diagnostics are produced.

Diagnostics can carry identifiers (i.e. error codes), source labels, suggestions, related diagnostics, causes, and context. The core runtime library supports `no_std` (with `alloc`), with certain features gated behind `std`, e.g. terminal reporting, panic hooks, global registration.

## Installation

```toml
[dependencies]
miden-diagnostics = "1.0"
```

The default configuration enables the `std` and `derive` features, for a `no_std` build, you must use `default-features = false`:

```toml
[dependencies]
miden-diagnostics = { version = "1.0", default-features = false, features = ["derive"] }
```

| Feature | Default | Purpose |
| --- | --- | --- |
| `std` | yes | Terminal emission, rich application termination, I/O emitters, and panic-hook support |
| `derive` | yes | Re-exports `#[derive(Diagnostic)]` |
| `simd` | no | Enables the renderer's SIMD-accelerated annotate-snippets path |
| `embed-explanations` | no | Retains authored long-form diagnostic explanations in the binary |
| `linked-registry` | no | Adds process-wide definition registration; implies `std` |

## Defining and deriving diagnostics

Use `diagnostic_codes!` to declare stable metadata separately from diagnostics themselves. Deriving `Diagnostic` then connects typed fields to the protocol:

```rust
use miden_diagnostics::{Diagnostic, SourceSpan};

miden_diagnostics::diagnostic_codes! {
    namespace = "calculator";
    catalog = DIAGNOSTICS;

    pub E1001 {
        summary: "expected an arithmetic expression",
        severity: Error,
    }
}

#[derive(Debug, Diagnostic)]
#[diagnostic(
    descriptor = E1001,
    message = "expected an expression, found {found}"
)]
struct ExpectedExpression {
    found: String,
    #[label(primary, "an integer or `(` was expected here")]
    span: SourceSpan,
}
```

`#[derive(Diagnostic)]` supports structs and enums, formatted messages, labels, 
notes, help, suggestions, related diagnostics, ordinary error sources, rich
diagnostic sources, transparent wrappers, and forwarding. For one-off
diagnostics, the `diagnostic!` and `report!` macros provide an ad hoc path;
`report!` is intentionally restricted to failures. It is recommended you define strongly-typed diagnostics in most cases though.

## Return fail-fast errors from library code

`miden_diagnostics::Result<T>` is an alias for `Result<T, Report>`. A `Report` owns its diagnostic and can accumulate application-independent context:

```rust
use miden_diagnostics::{Report, Result};

fn evaluate(expression: &Expr) -> Result<i64> {
    // ...
    Err(
        Report::new(DivisionByZero { span: expression.span() })
            .context("while evaluating the arithmetic expression"),
    )
}
```

This style is appropriate when an operation cannot produce a useful value
after its first error. `DiagnosticCollector::capture` lets a higher layer turn
such a `Result` failure back into a collected diagnostic when it needs to
continue coordinating other work.

## Collect diagnostics without discarding the value

Library-style analysis often needs to return warnings with a usable value, or
several errors from one pass. Return an `Outcome<T>` for that case:

```rust
use miden_diagnostics::{DefaultFailurePolicy, Outcome};

let parsed: Outcome<Option<Expr>> = parser::parse(source, input);

match parsed.into_result(&DefaultFailurePolicy) {
    Ok(outcome) => {
        // The default policy found no error. Warnings remain in
        // outcome.diagnostics and the value is safe to consume.
        use_expression(outcome.value);
    }
    Err(outcome) => {
        // The same value and diagnostics are retained for recovery or
        // reporting; policy conversion does not erase either one.
        recover(outcome);
    }
}
```

`Outcome::into_result` is an explicit policy boundary. The default policy
fails on errors; `WarningsAsErrors` also fails on warnings, and applications
can implement `FailurePolicy` for their own metadata rules. The conversion
returns the original `Outcome` in either branch.

`DiagnosticCollector` accepts typed or owned diagnostics, enforces optional
limits, preserves insertion order, merges finalized sets, and captures
`Result` failures:

```rust
let mut diagnostics = miden_diagnostics::DiagnosticCollector::new();
let _ = diagnostics.add(a_warning);
let value = diagnostics.capture(evaluate(&expression));

let outcome = miden_diagnostics::Outcome {
    value,
    diagnostics: diagnostics.finish(),
};
```

## Report diagnostics from an application

With the `std` feature, an application can return `ExitWithOutcome<T>` from `main`. `Outcome::into_exit` emits every diagnostic to stderr and chooses the process status using the configured failure policy:

```rust
use miden_diagnostics::{ExitWithOutcome, Outcome, SourceMap};

fn finish(
    outcome: Outcome<Option<i64>>,
    sources: SourceMap,
) -> ExitWithOutcome<Option<i64>> {
    outcome.into_exit().with_sources(sources)
}
```

The default policy exits successfully for warnings and unsuccessfully for
errors. Use `.with_policy(...)` to replace it or `.with_terminal_policy(...)`
to control color, Unicode, hyperlinks, and terminal-width detection.
Preparation or emission failures use a fixed fallback message and force a
failure exit.

Applications that need a buffer, protocol adapter, or custom destination can
prepare a `DiagnosticSet` with a `SourceProvider`, then pass the resulting
`PreparedSet` to `FmtEmitter`, `IoEmitter`, or a custom `Emitter`.
Preparation snapshots all semantic data before presentation.

To rich-render a single `OwnedDiagnostic` or `Report`, use its borrowing
display adapter. Sources attached to the diagnostic are used automatically;
session spans require an explicit session source provider:

```rust
println!("{}", report.display());

println!(
    "{}",
    report
        .display_with_sources(&sources)
        .with_config(miden_diagnostics::TerminalPolicy::default().resolve_stdout()),
);
```

The adapter's normal `Display` implementation safely degrades preparation or
rendering failures. Use `.try_render()` when those failures must remain
observable. Ordinary `Report::Display` stays concise for error chains, logs,
and `Error::to_string()`.

## Register and explain diagnostic codes

Diagnostic registration provides a mechanism for a few useful features:

* The ability to query a list of all known diagnostics
* `rustc --explain CODE`-like in-depth explanation of diagnostics

By default, a diagnostic registry requires explicit composition to build:

```rust
use miden_diagnostics::{DiagnosticDescriptor, StaticRegistry};

static CATALOGS: &[&[&DiagnosticDescriptor]] = &[parser::DIAGNOSTICS, eval::DIAGNOSTICS];
static REGISTRY: StaticRegistry<'static> = StaticRegistry::from_slices(CATALOGS);

REGISTRY.validate().expect("diagnostic codes must be unique");
let descriptor = REGISTRY.lookup("calculator/E1001")?;
```

`StaticRegistry` preserves catalog order, validates canonical-code uniqueness,
and supports canonical or uniquely resolvable short-code lookup. Matching the
descriptor's `Explanation` distinguishes text that was never provided, text
that was authored but omitted from this build, and embedded text.
Long explanations are lookup-only and are not appended to normal diagnostic
reports.

The optional `linked-registry` feature can collect definitions across a
native linked application. It is deliberately not the default: dead-code
elimination may require an application-owned reference to contributor crates,
raw `wasm32-unknown-unknown` hosts must run their guarded constructor path
before the first query, and the graph must use one runtime package/version.
Prefer `StaticRegistry` when portability or explicit composition matters.

## Use the runtime without `std`

With default features disabled, the core diagnostic protocol, owned reports,
collection, policy conversion, source model, preparation, explicit registry,
and formatting renderer remain available. The consumer supplies allocation
support and chooses how prepared diagnostics are transported or displayed.
The `derive` feature is independent of `std`, so typed diagnostics can still
be generated for `no_std` targets.

Process APIs such as `ExitWithOutcome`, terminal detection, `IoEmitter`, the
panic hook, and the linked registry require `std`. The repository's
`wasm32-unknown-unknown` consumers are executed with the Rust-native `wasmi`
runtime.

## In-depth example

The [arithmetic example](crates/diagnostics/examples/arithmetic/main.rs)
combines all of the the pieces above in one small application:

- the lexer emits multiple recoverable diagnostics through `DiagnosticSink`;
- the parser returns `Outcome<Option<Expr>>`;
- `Outcome::into_result` gates evaluation using `DefaultFailurePolicy`;
- the evaluator returns `miden_diagnostics::Result<i64>` with contextual
  `Report` failures;
- the application merges and captures diagnostics, registers every code in a
  `StaticRegistry`, attaches a `SourceMap`, and returns
  `Outcome::into_exit()`.

Run it with:

```console
cargo run -p miden-diagnostics --example arithmetic -- "10 / (3 - 3)"
cargo run -p miden-diagnostics --example arithmetic -- "007 + 1"
cargo run -p miden-diagnostics --example arithmetic -- --list-diagnostics
```

Smaller examples cover [fail-fast reports](crates/diagnostics/examples/fail_fast.rs),
[warning-only collection](crates/diagnostics/examples/warning_only.rs), and
[explicit explanation lookup](crates/diagnostics/examples/explain.rs).

Licensed under either [Apache License 2.0](LICENSE-APACHE) or
[MIT](LICENSE-MIT).
