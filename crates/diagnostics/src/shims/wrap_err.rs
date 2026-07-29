use alloc::string::ToString;
use core::fmt;

use crate::{Diagnostic, Report};

pub trait WrapErr<T, E>: _private::Sealed {
    /// Wrap the error value with a new adhoc error
    #[track_caller]
    fn wrap_err<D>(self, msg: D) -> Result<T, Report>
    where
        D: fmt::Display + Send + Sync + 'static;

    /// Wrap the error value with a new adhoc error that is evaluated lazily
    /// only once an error does occur.
    #[track_caller]
    fn wrap_err_with<D, F>(self, f: F) -> Result<T, Report>
    where
        D: fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> D;

    /// Compatibility re-export of `wrap_err()` for interop with `anyhow`
    #[track_caller]
    fn context<D>(self, msg: D) -> Result<T, Report>
    where
        D: fmt::Display + Send + Sync + 'static;

    /// Compatibility re-export of `wrap_err_with()` for interop with `anyhow`
    #[track_caller]
    fn with_context<D, F>(self, f: F) -> Result<T, Report>
    where
        D: fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> D;
}

mod _private {
    use crate::Report;

    pub trait Sealed {}

    impl<T, E> Sealed for Result<T, E> where Report: From<E> {}
    impl<T> Sealed for Option<T> {}
}

impl<T> WrapErr<T, core::convert::Infallible> for Option<T> {
    fn wrap_err<D>(self, msg: D) -> Result<T, Report>
    where
        D: fmt::Display + Send + Sync + 'static,
    {
        match self {
            Some(t) => Ok(t),
            None => Err(Report::new(DisplayError(msg))),
        }
    }

    fn wrap_err_with<D, F>(self, msg: F) -> Result<T, Report>
    where
        D: fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> D,
    {
        match self {
            Some(t) => Ok(t),
            None => Err(Report::new(DisplayError(msg()))),
        }
    }

    fn context<D>(self, msg: D) -> Result<T, Report>
    where
        D: fmt::Display + Send + Sync + 'static,
    {
        self.wrap_err(msg)
    }

    fn with_context<D, F>(self, msg: F) -> Result<T, Report>
    where
        D: fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> D,
    {
        self.wrap_err_with(msg)
    }
}

impl<T, E> WrapErr<T, E> for Result<T, E>
where
    Report: From<E>,
{
    fn wrap_err<D>(self, msg: D) -> Result<T, Report>
    where
        D: fmt::Display + Send + Sync + 'static,
    {
        match self {
            Ok(t) => Ok(t),
            Err(e) => Err(Report::from(e).context(msg.to_string())),
        }
    }

    fn wrap_err_with<D, F>(self, msg: F) -> Result<T, Report>
    where
        D: fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> D,
    {
        match self {
            Ok(t) => Ok(t),
            Err(e) => Err(Report::from(e).context(msg().to_string())),
        }
    }

    #[inline]
    fn context<D>(self, msg: D) -> Result<T, Report>
    where
        D: fmt::Display + Send + Sync + 'static,
    {
        self.wrap_err(msg)
    }

    #[inline]
    fn with_context<D, F>(self, msg: F) -> Result<T, Report>
    where
        D: fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> D,
    {
        self.wrap_err_with(msg)
    }
}

#[repr(transparent)]
struct DisplayError<M>(M);

impl<M> fmt::Debug for DisplayError<M>
where
    M: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl<M> fmt::Display for DisplayError<M>
where
    M: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl<M> core::error::Error for DisplayError<M> where M: fmt::Display + 'static {}
impl<M> Diagnostic for DisplayError<M>
where
    M: fmt::Display + 'static,
{
    fn message(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_fmt(format_args!("{}", self.0))
    }
}
