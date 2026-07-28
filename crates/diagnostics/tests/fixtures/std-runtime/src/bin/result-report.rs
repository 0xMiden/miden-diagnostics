use miden_diagnostics::{Report, Result};
use std_runtime::{MissingSource, PrepareFailure, rich_report};

fn main() -> Result<()> {
    let report = match std::env::args().nth(1).as_deref() {
        Some("rich") => rich_report(),
        Some("missing-source") => Report::new(MissingSource),
        Some("prepare-failure") => Report::new(PrepareFailure),
        other => panic!("unknown result-report mode: {other:?}"),
    };
    Err(report)
}
