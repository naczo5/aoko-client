//! One error type for every platform call.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformError {
    /// The OS denied access (permissions, protected process, sandbox).
    PermissionDenied { detail: String },
    /// The capability does not exist on this OS or session.
    Unsupported { detail: String },
    /// The target disappeared between enumeration and use.
    NotFound { detail: String },
    /// A Win32 / X11 / socket call failed.
    Os {
        operation: &'static str,
        detail: String,
    },
}

impl PlatformError {
    #[must_use]
    pub fn permission_denied(detail: impl Into<String>) -> Self {
        Self::PermissionDenied {
            detail: detail.into(),
        }
    }

    #[must_use]
    pub fn unsupported(detail: impl Into<String>) -> Self {
        Self::Unsupported {
            detail: detail.into(),
        }
    }

    #[must_use]
    pub fn not_found(detail: impl Into<String>) -> Self {
        Self::NotFound {
            detail: detail.into(),
        }
    }

    #[must_use]
    pub fn os(operation: &'static str, detail: impl Into<String>) -> Self {
        Self::Os {
            operation,
            detail: detail.into(),
        }
    }

    /// Text suitable for a capability banner or a log line.
    #[must_use]
    pub fn user_facing(&self) -> String {
        match self {
            Self::PermissionDenied { detail } => {
                format!("Permission denied: {detail}")
            }
            Self::Unsupported { detail } => format!("Not supported here: {detail}"),
            Self::NotFound { detail } => format!("Not found: {detail}"),
            Self::Os { operation, detail } => format!("{operation} failed: {detail}"),
        }
    }
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.user_facing())
    }
}

impl std::error::Error for PlatformError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_denied_is_actionable() {
        let err = PlatformError::permission_denied("uinput needs group input");
        assert_eq!(
            err.user_facing(),
            "Permission denied: uinput needs group input"
        );
    }

    #[test]
    fn os_error_names_the_operation() {
        let err = PlatformError::os("SetWindowsHookExW", "access denied");
        assert!(err.user_facing().starts_with("SetWindowsHookExW failed:"));
    }
}
