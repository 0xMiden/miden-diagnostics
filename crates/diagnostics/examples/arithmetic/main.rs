mod diagnostics;
mod evaluator;
mod lexer;
mod parser;
mod syntax;

use miden_diagnostics::{
    DefaultFailurePolicy, DiagnosticCollector, ExitWithOutcome, Outcome, SourceMap, SourceNamespace,
};

struct Session {
    outcome: Outcome<Option<i64>>,
    sources: SourceMap,
}

fn analyze(input: String) -> Session {
    let mut sources = SourceMap::new(SourceNamespace(1));
    let source = sources
        .insert("<expression>", input.clone(), None)
        .expect("the command-line expression must fit the u32 source model");

    let parsed = parser::parse(source, &input);
    // Parsing can recover a value alongside diagnostics. Policy conversion
    // keeps both, while making the application's "safe to evaluate" decision
    // explicit.
    let (expression, front_end_diagnostics) = match parsed.into_result(&DefaultFailurePolicy) {
        Ok(parsed) => (parsed.value, parsed.diagnostics),
        Err(parsed) => (None, parsed.diagnostics),
    };
    let mut diagnostics = DiagnosticCollector::new();
    let _ = diagnostics.merge(front_end_diagnostics);
    // The evaluator is deliberately fail-fast. Capture promotes its Report
    // back into this application-level diagnostic collection.
    let value = expression
        .and_then(|expression| diagnostics.capture(evaluator::evaluate(source, &expression)));

    Session {
        outcome: Outcome {
            value,
            diagnostics: diagnostics.finish(),
        },
        sources,
    }
}

fn empty_outcome() -> Outcome<Option<i64>> {
    Outcome {
        value: None,
        diagnostics: DiagnosticCollector::new().finish(),
    }
}

fn main() -> ExitWithOutcome<Option<i64>> {
    diagnostics::REGISTRY
        .validate()
        .expect("the arithmetic diagnostic catalog must be valid");

    let input = std::env::args().nth(1).unwrap_or_else(|| String::from("1 + 2 * 3"));
    if input == "--list-diagnostics" {
        for descriptor in diagnostics::REGISTRY.iter() {
            println!("{}: {}", descriptor.code, descriptor.summary);
        }
        return empty_outcome().into_exit();
    }

    let Session { outcome, sources } = analyze(input);
    if let Some(value) = outcome.value {
        println!("result: {value}");
    }
    outcome.into_exit().with_sources(sources)
}
