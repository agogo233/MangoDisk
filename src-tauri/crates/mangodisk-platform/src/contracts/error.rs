use std::{error::Error, fmt};

/// Stable error categories exposed by platform contracts.
///
/// Diagnostics remain native and English for logs. Product adapters localize
/// the code instead of interpreting diagnostic text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformErrorCode {
    AccessDenied,
    UserCancelled,
    ItemChanged,
    InvalidData,
    InvalidPath,
    Io,
    OperationFailed,
    Unsupported,
}

impl PlatformError {
    pub fn item_changed(diagnostic: impl Into<String>) -> Self {
        Self::new(PlatformErrorCode::ItemChanged, diagnostic)
    }
}

#[derive(Debug, Clone)]
pub struct PlatformError {
    code: PlatformErrorCode,
    diagnostic: String,
}

impl PlatformError {
    pub fn new(code: PlatformErrorCode, diagnostic: impl Into<String>) -> Self {
        Self {
            code,
            diagnostic: diagnostic.into(),
        }
    }

    pub fn code(&self) -> PlatformErrorCode {
        self.code
    }

    pub fn diagnostic(&self) -> &str {
        &self.diagnostic
    }

    /// Returns the stable diagnostic payload for hashing or structured logs.
    pub fn as_bytes(&self) -> &[u8] {
        self.diagnostic.as_bytes()
    }

    pub fn operation_failed(diagnostic: impl Into<String>) -> Self {
        Self::new(PlatformErrorCode::OperationFailed, diagnostic)
    }

    pub fn invalid_path(diagnostic: impl Into<String>) -> Self {
        Self::new(PlatformErrorCode::InvalidPath, diagnostic)
    }

    pub fn io(operation: &'static str, error: &std::io::Error) -> Self {
        let code = match error.kind() {
            std::io::ErrorKind::PermissionDenied => PlatformErrorCode::AccessDenied,
            std::io::ErrorKind::InvalidData | std::io::ErrorKind::InvalidInput => {
                PlatformErrorCode::InvalidData
            }
            _ => PlatformErrorCode::Io,
        };
        Self::new(code, format!("{operation}: {:?}", error.kind()))
    }
}

impl fmt::Display for PlatformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.diagnostic)
    }
}

impl Error for PlatformError {}

impl From<String> for PlatformError {
    fn from(diagnostic: String) -> Self {
        Self::operation_failed(diagnostic)
    }
}

impl From<&str> for PlatformError {
    fn from(diagnostic: &str) -> Self {
        Self::operation_failed(diagnostic)
    }
}

pub type PlatformResult<T> = Result<T, PlatformError>;
