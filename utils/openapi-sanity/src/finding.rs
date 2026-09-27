use std::fmt;

use proc_macro2::Span;

/// A problem the caller decides what to do with.
///
/// The crate never prints and never fails: an unparsable `routes![]` entry, a
/// route attribute without a URI literal and a syntax error all surface as
/// findings carrying a file label and, when the input provided a span, a 1-based
/// line. Build warnings, gate failures and CLI output are the caller's business,
/// which keeps this crate usable from a build script, a test and a binary
/// alike.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Caller-supplied label of the analyzed file. The crate performs no I/O, so
    /// this is a string for diagnostics, not a path it has opened.
    pub file: String,
    /// 1-based line, or `None` when the input carried no usable span.
    pub line: Option<usize>,
    /// What is wrong, phrased so it can be printed unchanged.
    pub message: String,
}

impl Finding {
    /// A finding anchored at a span of the analyzed file.
    pub(crate) fn at(file: &str, span: Span, message: impl Into<String>) -> Self {
        Self::on_line(file, span.start().line, message)
    }

    /// A finding anchored at a 1-based line of the analyzed file.
    pub(crate) fn on_line(file: &str, line: usize, message: impl Into<String>) -> Self {
        Self {
            file: file.to_string(),
            // `Span::call_site()` and friends report line 0, which is "unknown"
            // rather than "first line".
            line: (line > 0).then_some(line),
            message: message.into(),
        }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{line}: {}", self.file, self.message),
            None => write!(f, "{}: {}", self.file, self.message),
        }
    }
}
