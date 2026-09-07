mod diagnostics;
mod evaluator;
mod lexer;
mod parser;
mod syntax;

use miden_diagnostics::{
    DiagnosticCollector, ExitWithOutcome, Outcome, SourceMap, SourceNamespace,
};

struct Session {
    outcome: Outcome<i64>,
    sources: SourceMap,
}

fn analyze(input: String) -> Session {
    let mut sources = SourceMap::new(SourceNamespace::new_unchecked(1));
    let source = sources
        .insert("<expression>", input.clone(), None)
        .expect("the command-line expression must fit the u32 source model");

    // Parsing can recover a value alongside diagnostics. Policy conversion
    // keeps both, while making the application's "safe to evaluate" decision
    // explicit.
    //
    // The evaluator is deliberately fail-fast. Capture promotes its Report
    // back into this application-level diagnostic collection.
    let outcome = parser::parse(source, &input).and_then(|expr, collector| {
        collector.capture(evaluator::evaluate(source, &expr)).ok_or(())
    });

    Session { outcome, sources }
}

fn empty_outcome() -> Outcome<i64> {
    Outcome {
        result: Ok(0),
        diagnostics: DiagnosticCollector::new().finish(),
    }
}

fn main() -> ExitWithOutcome<i64> {
    diagnostics::REGISTRY
        .validate()
        .expect("the arithmetic diagnostic catalog must be valid");

    let input = std::env::args().nth(1).unwrap_or_else(|| String::from("1 + 2 * 3"));
    if input == "--list-diagnostics" {
        for descriptor in diagnostics::REGISTRY.iter() {
            println!("{}: {}", descriptor.code, descriptor.summary);
        }
        return empty_outcome().into();
    }

    let Session { outcome, sources } = analyze(input);
    if let Ok(value) = outcome.result
        && outcome.is_ok()
    {
        println!("result: {value}");
    }
    ExitWithOutcome::from(outcome).with_sources(sources)
}
