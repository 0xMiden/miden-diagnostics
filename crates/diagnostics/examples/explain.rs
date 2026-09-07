use std::{env, process::ExitCode};

use miden_diagnostics::{Explanation, LookupError, StaticRegistry};

mod parser {
    miden_diagnostics::diagnostic_codes! {
        namespace = "example::parser";
        catalog = DIAGNOSTICS;

        pub E1000 {
            summary: "unexpected token",
            severity: Error,
            explanation: include_str!("explain/E1000.md"),
            documentation_url: "https://example.com/diagnostics/E1000",
        }

        pub W2000 {
            summary: "binding shadows an earlier binding",
            severity: Warning,
            documentation_url: "https://example.com/diagnostics/W2000",
        }
    }
}

mod typeck {
    miden_diagnostics::diagnostic_codes! {
        namespace = "example::typeck";
        catalog = DIAGNOSTICS;

        pub E1000 {
            summary: "incompatible types",
            severity: Error,
        }
    }
}

static CATALOGS: &[&[&miden_diagnostics::DiagnosticDescriptor]] =
    &[parser::DIAGNOSTICS, typeck::DIAGNOSTICS];
static REGISTRY: StaticRegistry<'static> = StaticRegistry::from_slices(CATALOGS);

fn main() -> ExitCode {
    let Some(code) = env::args().nth(1) else {
        eprintln!("usage: cargo run --example explain -- <namespace/code|short-code>");
        return ExitCode::from(64);
    };

    let descriptor = match REGISTRY.lookup(&code) {
        Ok(descriptor) => descriptor,
        Err(LookupError::UnknownCode) => {
            eprintln!("unknown diagnostic code: {code}");
            return ExitCode::from(2);
        }
        Err(LookupError::AmbiguousCode { first, second }) => {
            eprintln!("ambiguous diagnostic code {code}: {first}, {second}");
            return ExitCode::from(2);
        }
        Err(LookupError::InvalidRegistry(error)) => {
            eprintln!("invalid diagnostic registry: {error}");
            return ExitCode::from(70);
        }
    };

    println!("{}: {}", descriptor.code, descriptor.summary);
    if let Some(url) = descriptor.documentation_url {
        println!("documentation: {url}");
    }
    match descriptor.explanation {
        Explanation::Embedded(markdown) => {
            println!("\n{markdown}");
            ExitCode::SUCCESS
        }
        Explanation::NotProvided => {
            println!("explanation: not provided");
            ExitCode::SUCCESS
        }
        Explanation::NotEmbedded => {
            eprintln!("explanation: not embedded in this build");
            ExitCode::from(3)
        }
    }
}
