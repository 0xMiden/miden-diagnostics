use core::fmt;

use miden_diagnostics::{Diagnostic, Report, Result};

#[derive(Debug)]
struct ParseFailure;

impl Diagnostic for ParseFailure {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("input could not be parsed")
    }
}

fn parse() -> Result<()> {
    Err(Report::new(ParseFailure))
}

fn main() {
    let report = parse().expect_err("the fail-fast example must return a report");
    assert_eq!(report.to_string(), "input could not be parsed");
}
