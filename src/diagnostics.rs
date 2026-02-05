//! Shared diagnostics helpers for consistent, colored output.

use colored::Colorize;

/// Format a warning message with a colored prefix.
pub fn warn(message: impl AsRef<str>) {
    eprintln!("{} {}", "WARN".yellow().bold(), message.as_ref());
}

/// Format an error message with a colored prefix.
pub fn error_message(message: impl AsRef<str>) -> String {
    format!("{} {}", "ERROR".red().bold(), message.as_ref())
}
