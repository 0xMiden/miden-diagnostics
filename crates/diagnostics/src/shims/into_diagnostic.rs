use crate::Report;

/// Convenience trait that adds a [`into_diagnostic`](IntoDiagnostic::into_diagnostic) method that
/// converts a type implementing [`core::error::Error`] to a [`Result<T, Report>`].
///
/// ## Warning
///
/// Calling this on a type implementing [`Diagnostic`] will reduce it to the common denominator of
/// [`core::error::Error`]. Meaning all extra information provided by [`Diagnostic`] will be
/// inaccessible. If you have a type implementing [`Diagnostic`] consider simply returning it or
/// using [`Into`] or the [`Try`](core::ops::Try) operator (`?`).
pub trait IntoDiagnostic<T, E> {
    /// Converts [`Result`] types that return regular [`core::error::Error`]s into a [`Result`]
    /// that returns a [`Diagnostic`].
    fn into_diagnostic(self) -> Result<T, Report>;
}

impl<T, E: core::error::Error + Send + Sync + 'static> IntoDiagnostic<T, E> for Result<T, E> {
    fn into_diagnostic(self) -> Result<T, Report> {
        self.map_err(Report::from_error)
    }
}

#[cfg(test)]
mod tests {
    use std::{error::Error, fmt, io, string::ToString};

    use super::*;

    #[derive(Debug)]
    struct TestError(io::Error);

    impl fmt::Display for TestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "testing, testing...")
        }
    }

    impl Error for TestError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn diagnostic_error() {
        let inner_error = io::Error::other("halt and catch fire");
        let outer_error: Result<(), _> = Err(TestError(inner_error));

        let diagnostic_error = outer_error.into_diagnostic().unwrap_err();

        assert_eq!(diagnostic_error.to_string(), "testing, testing...");
        assert_eq!(diagnostic_error.source().unwrap().to_string(), "halt and catch fire");
    }
}
