# miden-diagnostics

This is the runtime crate for Miden's structured diagnostics. It is designed
for both library and application code: libraries describe and transport
diagnostics without installing global handlers, while applications decide
failure policy, source resolution, rendering, emission, and process exit.

The crate is `no_std` with `alloc`. Its default features enable `std` and
`derive` (which provides `#[derive(Diagnostic)]`).

## Structure

The public API is organized around a few layers:

- `Diagnostic`, descriptor types, and `diagnostic_codes!` define semantic
  diagnostic data and catalogs.
- `OwnedDiagnostic`, `Report`, and the `Result<T>` alias provide owned,
  transport-safe fail-fast errors and context.
- `DiagnosticCollector`, `DiagnosticSet`, and `Outcome<T>` collect warnings
  and errors without forcing an either/or result model.
- `SourceMap` and the `SourceProvider` traits resolve session and
  diagnostic-attached source universes.
- snapshot preparation validates and owns messages before
  `AnnotateRenderer` and the emitter traits present them.
- `OwnedDiagnostic::display` and `Report::display` rich-render a single
  occurrence without consuming it; session spans can be supplied with
  `display_with_sources`.
- `StaticRegistry` explicitly composes diagnostic catalogs. The optional
  linked registry provides process-wide diagnostic registration.
- with the `std` feature enabled, `ExitWithOutcome`, terminal emitters, and 
  panic-hook support provide application-level integration.

Collection, failure policy, and presentation are intentionally independent.
`Outcome::into_result` evaluates policy without losing the outcome;
`Outcome::into_exit` is the rich `std` application boundary.

## Features

| Feature | Default | Adds |
| --- | --- | --- |
| `std` | yes | Terminal/I/O emission, rich termination, and panic-hook support |
| `derive` | yes | The `Diagnostic` derive re-export |
| `simd` | no | SIMD support in the annotate-snippets renderer |
| `embed-explanations` | no | Long-form explanation text in the artifact |
| `linked-registry` | no | Native definition registration; implies `std` |

Explicit `StaticRegistry` composition is the default for portability. Linked
registration is sensitive to the linker for a given target, so applications should opt into it deliberately. See the docs for the `inventory` crate for more information on when it is appropriate to rely on global registration.

## Getting oriented

Start with the repository's
[arithmetic example](examples/arithmetic/main.rs) for a complete lexer,
parser, evaluator, collector, registry, source-map, and application-reporting
flow. The smaller [fail-fast](examples/fail_fast.rs),
[warning-only](examples/warning_only.rs), and
[explanation lookup](examples/explain.rs) examples isolate individual use
cases.

The [workspace README](../../README.md) contains installation instructions and
usage-oriented examples. Detailed implementation contracts live in the
workspace architecture document.

This crate is licensed under Apache-2.0 OR MIT.
