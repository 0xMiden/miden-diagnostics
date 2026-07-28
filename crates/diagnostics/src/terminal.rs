use core::num::NonZeroU16;
use std::{
    env,
    ffi::OsStr,
    io::{self, IsTerminal, Write},
};

use crate::{
    AnnotateRenderer, EmissionFailure, EmissionStatus, EmissionSummary, Emitter, IoEmissionError,
    IoEmitter, PreparedDiagnostic, PreparedSet, RenderConfig,
};

/// An explicit override or environment-derived terminal choice.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TerminalChoice {
    #[default]
    Auto,
    Always,
    Never,
}

/// An explicit render width or a width derived from the terminal environment.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TerminalWidth {
    #[default]
    Auto,
    Fixed(NonZeroU16),
}

/// Controls terminal capabilities for stderr rendering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalPolicy {
    pub styled: TerminalChoice,
    pub unicode: TerminalChoice,
    pub hyperlinks: TerminalChoice,
    pub width: TerminalWidth,
}

impl TerminalPolicy {
    pub const DEFAULT: Self = Self {
        styled: TerminalChoice::Auto,
        unicode: TerminalChoice::Auto,
        hyperlinks: TerminalChoice::Auto,
        width: TerminalWidth::Auto,
    };

    /// Resolves this policy once against stderr and the current environment.
    pub fn resolve(self) -> RenderConfig {
        self.resolve_with(TerminalInputs::detect())
    }

    fn resolve_with(self, inputs: TerminalInputs) -> RenderConfig {
        let usable_terminal = inputs.is_terminal && !inputs.term_is_dumb;
        RenderConfig {
            styled: resolve_choice(self.styled, usable_terminal && !inputs.no_color_requested),
            unicode: resolve_choice(self.unicode, usable_terminal && inputs.unicode_locale),
            // Automatic hyperlink detection is intentionally conservative.
            hyperlinks: resolve_choice(self.hyperlinks, false),
            width: match self.width {
                TerminalWidth::Fixed(width) => usize::from(width.get()),
                TerminalWidth::Auto => inputs
                    .columns
                    .filter(|_| inputs.is_terminal)
                    .map_or(RenderConfig::DEFAULT.width, usize::from),
            },
            ..RenderConfig::DEFAULT
        }
    }
}

impl Default for TerminalPolicy {
    fn default() -> Self {
        Self::DEFAULT
    }
}

fn resolve_choice(choice: TerminalChoice, automatic: bool) -> bool {
    match choice {
        TerminalChoice::Auto => automatic,
        TerminalChoice::Always => true,
        TerminalChoice::Never => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TerminalInputs {
    is_terminal: bool,
    term_is_dumb: bool,
    no_color_requested: bool,
    unicode_locale: bool,
    columns: Option<u16>,
}

impl TerminalInputs {
    fn detect() -> Self {
        let is_terminal = io::stderr().is_terminal();
        let term_is_dumb =
            env::var_os("TERM").as_deref().is_some_and(|term| eq_ascii_case(term, "dumb"));
        let no_color_requested =
            env::var_os("NO_COLOR").as_deref().is_some_and(|value| !value.is_empty());
        let unicode_locale = unicode_locale();
        let columns = parse_columns(env::var_os("COLUMNS").as_deref());
        Self {
            is_terminal,
            term_is_dumb,
            no_color_requested,
            unicode_locale,
            columns,
        }
    }
}

fn parse_columns(value: Option<&OsStr>) -> Option<u16> {
    value
        .and_then(OsStr::to_str)
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|columns| *columns != 0)
}

fn eq_ascii_case(value: &OsStr, expected: &str) -> bool {
    value.to_str().is_some_and(|value| value.eq_ignore_ascii_case(expected))
}

#[cfg(windows)]
fn unicode_locale() -> bool {
    io::stderr().is_terminal()
}

#[cfg(not(windows))]
fn unicode_locale() -> bool {
    ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .filter_map(env::var_os)
        .find(|value| !value.is_empty())
        .and_then(|value| value.into_string().ok())
        .is_some_and(|locale| {
            let locale = locale.to_ascii_uppercase();
            locale.contains("UTF-8") || locale.contains("UTF8")
        })
}

/// Emits diagnostics to stderr using capabilities resolved at construction.
pub struct StderrEmitter {
    renderer: AnnotateRenderer,
}

impl StderrEmitter {
    pub fn new(policy: TerminalPolicy) -> Self {
        Self {
            renderer: AnnotateRenderer::new(policy.resolve()),
        }
    }

    pub const fn renderer(&self) -> &AnnotateRenderer {
        &self.renderer
    }
}

impl Default for StderrEmitter {
    fn default() -> Self {
        Self::new(TerminalPolicy::default())
    }
}

impl Emitter for StderrEmitter {
    type Error = IoEmissionError;

    fn emit(&mut self, diagnostic: &PreparedDiagnostic<'_>) -> Result<EmissionStatus, Self::Error> {
        let stderr = io::stderr();
        let lock = stderr.lock();
        IoEmitter::new(lock, self.renderer).emit(diagnostic)
    }

    fn emit_set(
        &mut self,
        diagnostics: &PreparedSet<'_>,
    ) -> Result<EmissionSummary, EmissionFailure<Self::Error>> {
        let stderr = io::stderr();
        let lock = stderr.lock();
        IoEmitter::new(lock, self.renderer).emit_set(diagnostics)
    }
}

pub(crate) const PREPARATION_FALLBACK: &[u8] =
    b"error: diagnostic preparation failed; rich output unavailable\n";
pub(crate) const EMISSION_FALLBACK: &[u8] =
    b"\nerror: diagnostic emission failed; rich output incomplete\n";
pub(crate) const PANIC_FALLBACK: &[u8] = b"fatal: diagnostic panic report unavailable\n";

pub(crate) fn write_stderr_fallback(message: &[u8]) {
    let stderr = io::stderr();
    let mut lock = stderr.lock();
    let _ = lock.write_all(message);
    let _ = lock.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    const NON_TERMINAL: TerminalInputs = TerminalInputs {
        is_terminal: false,
        term_is_dumb: false,
        no_color_requested: false,
        unicode_locale: true,
        columns: Some(90),
    };

    #[test]
    fn automatic_policy_is_conservative_off_terminal() {
        assert_eq!(TerminalPolicy::DEFAULT.resolve_with(NON_TERMINAL), RenderConfig::DEFAULT);
    }

    #[test]
    fn automatic_policy_honors_terminal_constraints() {
        let resolved = TerminalPolicy::DEFAULT.resolve_with(TerminalInputs {
            is_terminal: true,
            term_is_dumb: false,
            no_color_requested: false,
            unicode_locale: true,
            columns: Some(90),
        });
        assert!(resolved.styled);
        assert!(resolved.unicode);
        assert!(!resolved.hyperlinks);
        assert_eq!(resolved.width, 90);

        let dumb = TerminalPolicy::DEFAULT.resolve_with(TerminalInputs {
            is_terminal: true,
            term_is_dumb: true,
            ..NON_TERMINAL
        });
        assert!(!dumb.styled);
        assert!(!dumb.unicode);

        let no_color = TerminalPolicy::DEFAULT.resolve_with(TerminalInputs {
            is_terminal: true,
            no_color_requested: true,
            ..NON_TERMINAL
        });
        assert!(!no_color.styled);
        assert!(no_color.unicode);
    }

    #[test]
    fn automatic_width_parser_rejects_every_invalid_shape() {
        assert_eq!(parse_columns(Some(OsStr::new("1"))), Some(1));
        assert_eq!(parse_columns(Some(OsStr::new("65535"))), Some(65535));
        for invalid in ["", "0", "-1", "not-a-number", "65536", "999999999999999"] {
            assert_eq!(parse_columns(Some(OsStr::new(invalid))), None);
        }

        let valid = TerminalPolicy::DEFAULT.resolve_with(TerminalInputs {
            is_terminal: true,
            columns: parse_columns(Some(OsStr::new("91"))),
            ..NON_TERMINAL
        });
        assert_eq!(valid.width, 91);
        let invalid = TerminalPolicy::DEFAULT.resolve_with(TerminalInputs {
            is_terminal: true,
            columns: parse_columns(Some(OsStr::new("65536"))),
            ..NON_TERMINAL
        });
        assert_eq!(invalid.width, RenderConfig::DEFAULT.width);
    }

    #[test]
    fn explicit_choices_override_detection() {
        let resolved = TerminalPolicy {
            styled: TerminalChoice::Always,
            unicode: TerminalChoice::Always,
            hyperlinks: TerminalChoice::Always,
            width: TerminalWidth::Fixed(NonZeroU16::new(72).unwrap()),
        }
        .resolve_with(NON_TERMINAL);
        assert_eq!(
            resolved,
            RenderConfig {
                styled: true,
                unicode: true,
                hyperlinks: true,
                width: 72,
                ..RenderConfig::DEFAULT
            }
        );
    }
}
