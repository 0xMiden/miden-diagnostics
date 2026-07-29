use miden_diagnostics::{Emitter, Report, Result, StdoutEmitter};
use std_runtime::{MissingSource, PrepareFailure, rich_report};

fn main() -> Result<()> {
    let mode = std::env::args().nth(1);
    if mode.as_deref() == Some("stdout-emitter") {
        let report = rich_report();
        let diagnostic = report.prepare_attached().map_err(Report::from_error)?;
        StdoutEmitter::default().emit(&diagnostic).map_err(Report::from_error)?;
        return Ok(());
    }

    let report = match mode.as_deref() {
        Some("rich") => rich_report(),
        Some("missing-source") => Report::new(MissingSource),
        Some("prepare-failure") => Report::new(PrepareFailure),
        other => panic!("unknown result-report mode: {other:?}"),
    };
    Err(report)
}
