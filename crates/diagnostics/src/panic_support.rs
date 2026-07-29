#[cfg(feature = "std")]
use alloc::{
    boxed::Box,
    string::{String, ToString},
};
#[cfg(feature = "std")]
use core::{cell::Cell, fmt};
#[cfg(feature = "std")]
use std::{
    io::{self, Write},
    panic::{self, PanicHookInfo},
    sync::Mutex,
};

use crate::Report;
#[cfg(feature = "std")]
use crate::{
    TerminalPolicy,
    terminal::{PANIC_FALLBACK, write_stderr_fallback},
};

/// Configuration captured by the process-global diagnostic panic hook.
#[cfg(feature = "std")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PanicHookOptions {
    pub terminal_policy: TerminalPolicy,
}

#[cfg(feature = "std")]
impl PanicHookOptions {
    pub const DEFAULT: Self = Self {
        terminal_policy: TerminalPolicy::DEFAULT,
    };

    pub const fn new(terminal_policy: TerminalPolicy) -> Self {
        Self { terminal_policy }
    }
}

#[cfg(feature = "std")]
impl Default for PanicHookOptions {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Why a process-global diagnostic panic hook could not be installed.
#[cfg(feature = "std")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallHookError {
    PanickingThread,
    IncompatibleOptions {
        installed: PanicHookOptions,
        requested: PanicHookOptions,
    },
}

#[cfg(feature = "std")]
impl fmt::Display for InstallHookError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PanickingThread => {
                formatter.write_str("cannot install a panic hook from a panicking thread")
            }
            Self::IncompatibleOptions { .. } => formatter
                .write_str("a diagnostic panic hook is already installed with different options"),
        }
    }
}

#[cfg(feature = "std")]
impl core::error::Error for InstallHookError {}

#[cfg(feature = "std")]
static INSTALLATION: Mutex<Option<PanicHookOptions>> = Mutex::new(None);

#[cfg(feature = "std")]
std::thread_local! {
    static HOOK_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

#[cfg(feature = "std")]
struct HookGuard;

#[cfg(feature = "std")]
impl HookGuard {
    fn enter() -> Option<Self> {
        HOOK_ACTIVE
            .try_with(|active| {
                if active.get() {
                    false
                } else {
                    active.set(true);
                    true
                }
            })
            .ok()
            .filter(|entered| *entered)
            .map(|_| Self)
    }
}

#[cfg(feature = "std")]
impl Drop for HookGuard {
    fn drop(&mut self) {
        let _ = HOOK_ACTIVE.try_with(|active| active.set(false));
    }
}

#[cfg(feature = "std")]
struct ReportPanic {
    _report: Report,
    framed_record: String,
}

/// Installs the process-global diagnostic panic hook.
///
/// Reinstalling identical options is idempotent. Applications must coordinate
/// this process-global resource with any other code that replaces panic hooks.
#[cfg(feature = "std")]
pub fn install_panic_hook(options: PanicHookOptions) -> core::result::Result<(), InstallHookError> {
    if std::thread::panicking() {
        return Err(InstallHookError::PanickingThread);
    }

    let mut installation = INSTALLATION.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(installed) = *installation {
        return if installed == options {
            Ok(())
        } else {
            Err(InstallHookError::IncompatibleOptions {
                installed,
                requested: options,
            })
        };
    }

    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| dispatch_hook(info, previous.as_ref())));
    *installation = Some(options);
    Ok(())
}

#[cfg(feature = "std")]
fn dispatch_hook(
    info: &PanicHookInfo<'_>,
    previous: &(dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static),
) {
    let Some(_guard) = HookGuard::enter() else {
        write_stderr_fallback(PANIC_FALLBACK);
        return;
    };

    let Some(payload) = info.payload().downcast_ref::<ReportPanic>() else {
        previous(info);
        return;
    };

    let stderr = io::stderr();
    let mut lock = stderr.lock();
    if lock.write_all(payload.framed_record.as_bytes()).is_err() || lock.flush().is_err() {
        drop(lock);
        write_stderr_fallback(PANIC_FALLBACK);
    }
}

#[cfg(feature = "std")]
fn installed_options() -> Option<PanicHookOptions> {
    *INSTALLATION.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(feature = "std")]
#[doc(hidden)]
#[track_caller]
pub fn panic_report(report: Report) -> ! {
    if std::thread::panicking() {
        write_stderr_fallback(PANIC_FALLBACK);
        std::process::abort();
    }

    let installed = installed_options();
    let options = installed.unwrap_or_default();
    let mut framed_record =
        report.display().with_config(options.terminal_policy.resolve()).to_string();
    framed_record.push('\n');
    match installed {
        Some(_) => panic::panic_any(ReportPanic {
            _report: report,
            framed_record,
        }),
        None => panic::panic_any(framed_record),
    }
}

#[cfg(not(feature = "std"))]
#[doc(hidden)]
#[track_caller]
pub fn panic_report(report: Report) -> ! {
    panic!("{report}")
}
