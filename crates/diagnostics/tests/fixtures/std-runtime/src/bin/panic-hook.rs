use core::{
    fmt,
    sync::atomic::{AtomicUsize, Ordering},
};
use std::{
    io::{self, Write},
    panic::{self, AssertUnwindSafe, PanicHookInfo},
    sync::{Arc, Barrier},
};

use miden_diagnostics::{
    Diagnostic, InstallHookError, PanicHookOptions, Report, TerminalChoice, TerminalPolicy,
    install_panic_hook, panic_report,
};
use std_runtime::{PrepareFailure, rich_report};

static MESSAGE_CALLS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
struct CountedDiagnostic;

impl Diagnostic for CountedDiagnostic {
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        assert!(!std::thread::panicking(), "message called while panicking");
        MESSAGE_CALLS.fetch_add(1, Ordering::SeqCst);
        out.write_str("counted diagnostic")
    }
}

#[derive(Debug)]
struct PanickingDiagnostic;

impl Diagnostic for PanickingDiagnostic {
    fn message(&self, _out: &mut dyn fmt::Write) -> fmt::Result {
        panic!("message formatter panicked")
    }
}

#[derive(Debug)]
struct Unrelated;

fn previous_hook(info: &PanicHookInfo<'_>) {
    let payload = if info.payload().is::<Unrelated>() {
        "unrelated"
    } else if let Some(message) = info.payload().downcast_ref::<String>() {
        message
    } else if let Some(message) = info.payload().downcast_ref::<&str>() {
        message
    } else {
        "unknown"
    };
    eprintln!("previous:{payload}");
}

fn install() {
    panic::set_hook(Box::new(previous_hook));
    install_panic_hook(PanicHookOptions::default()).unwrap();
}

fn main() {
    let mode = std::env::args().nth(1);
    match mode.as_deref() {
        Some("typed") => {
            println!(
                "profile={}",
                if cfg!(panic = "abort") {
                    "abort"
                } else {
                    "unwind"
                }
            );
            io::stdout().flush().unwrap();
            install();
            panic_report!(rich_report());
        }
        Some("typed-caught") => {
            install();
            let caught = panic::catch_unwind(AssertUnwindSafe(|| {
                panic_report!(Report::new(CountedDiagnostic));
            }));
            println!(
                "caught={} calls={}",
                caught.is_err(),
                MESSAGE_CALLS.load(Ordering::SeqCst)
            );
        }
        Some("unrelated") => {
            println!(
                "profile={}",
                if cfg!(panic = "abort") {
                    "abort"
                } else {
                    "unwind"
                }
            );
            io::stdout().flush().unwrap();
            install();
            panic::panic_any(Unrelated);
        }
        Some("formatted-report") => {
            install();
            panic!("{}", rich_report());
        }
        Some("install-state") => {
            panic::set_hook(Box::new(previous_hook));
            let options = PanicHookOptions::default();
            println!("first={:?}", install_panic_hook(options));
            println!("same={:?}", install_panic_hook(options));
            let different = PanicHookOptions::new(TerminalPolicy {
                styled: TerminalChoice::Always,
                ..TerminalPolicy::DEFAULT
            });
            println!("different={:?}", install_panic_hook(different));
            let _ = panic::catch_unwind(|| panic!("ordinary"));
        }
        Some("install-concurrent-same") => {
            panic::set_hook(Box::new(previous_hook));
            let barrier = Arc::new(Barrier::new(9));
            let mut threads = Vec::new();
            for _ in 0..8 {
                let barrier = Arc::clone(&barrier);
                threads.push(std::thread::spawn(move || {
                    barrier.wait();
                    install_panic_hook(PanicHookOptions::default())
                }));
            }
            barrier.wait();
            let results: Vec<_> = threads
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .collect();
            assert!(results.iter().all(Result::is_ok));
            println!("same-ok={}", results.len());
        }
        Some("install-concurrent-mixed") => {
            panic::set_hook(Box::new(previous_hook));
            let automatic = PanicHookOptions::default();
            let styled = PanicHookOptions::new(TerminalPolicy {
                styled: TerminalChoice::Always,
                ..TerminalPolicy::DEFAULT
            });
            let barrier = Arc::new(Barrier::new(9));
            let mut threads = Vec::new();
            for index in 0..8 {
                let requested = if index % 2 == 0 { automatic } else { styled };
                let barrier = Arc::clone(&barrier);
                threads.push((
                    requested,
                    std::thread::spawn(move || {
                        barrier.wait();
                        install_panic_hook(requested)
                    }),
                ));
            }
            barrier.wait();
            let results: Vec<_> = threads
                .into_iter()
                .map(|(requested, thread)| (requested, thread.join().unwrap()))
                .collect();
            let winner = results
                .iter()
                .find_map(|(_, result)| match result {
                    Err(InstallHookError::IncompatibleOptions { installed, .. }) => {
                        Some(*installed)
                    }
                    _ => None,
                })
                .expect("mixed installation must have an incompatible group");
            let mut winner_ok = 0;
            let mut incompatible = 0;
            for (requested, result) in results {
                if requested == winner {
                    assert_eq!(result, Ok(()));
                    winner_ok += 1;
                } else {
                    assert_eq!(
                        result,
                        Err(InstallHookError::IncompatibleOptions {
                            installed: winner,
                            requested,
                        })
                    );
                    incompatible += 1;
                }
            }
            let winner_name = if winner == automatic {
                "auto"
            } else {
                "styled"
            };
            println!(
                "mixed-winner={winner_name} winner-ok={winner_ok} incompatible={incompatible}"
            );
        }
        Some("delegation-location") => {
            panic::set_hook(Box::new(|info| {
                let location = info.location().expect("panic location must be available");
                eprintln!(
                    "delegated-location={}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                );
            }));
            install_panic_hook(PanicHookOptions::default()).unwrap();
            panic::panic_any(Unrelated);
        }
        Some("install-while-panicking") => {
            panic::set_hook(Box::new(|_| {
                eprintln!(
                    "install-during={:?}",
                    install_panic_hook(PanicHookOptions::default())
                );
            }));
            panic!("outer");
        }
        Some("pre-render-panics") => {
            install();
            panic_report!(Report::new(PanickingDiagnostic));
        }
        Some("prepare-failure") => {
            install();
            panic_report!(Report::new(PrepareFailure));
        }
        Some("uninstalled") => {
            panic::set_hook(Box::new(previous_hook));
            panic_report!(rich_report());
        }
        Some("panic-during-unwind") => {
            struct PanicOnDrop;
            impl Drop for PanicOnDrop {
                fn drop(&mut self) {
                    panic_report!(Report::new(CountedDiagnostic));
                }
            }
            install();
            let _drop = PanicOnDrop;
            panic!("begin unwind");
        }
        Some("check-panicking-error") => {
            assert_eq!(
                InstallHookError::PanickingThread.to_string(),
                "cannot install a panic hook from a panicking thread"
            );
        }
        other => panic!("unknown panic-hook mode: {other:?}"),
    }
}
