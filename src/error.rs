use std::fmt::Formatter;
use tokio::sync::mpsc::error::SendError;
use crate::levels::LogLevel;

pub enum LoggerError {
    InvalidRegion,
    InvalidLogGroup,
    InvalidLogStream,
    InvalidLogEvent,
    InvalidLogEventBatch,
    DescribeLogStreamError(rusoto_core::RusotoError<rusoto_logs::DescribeLogStreamsError>),
    SendError(SendError<LogLevel>),

    #[cfg(feature = "singleton")]
    Poisoned
}

fn format_logger_error(l: &LoggerError, fmt: &mut Formatter) -> std::fmt::Result {
    match l {
        LoggerError::InvalidRegion => {
            write!(fmt, "Invalid Region")
        }
        LoggerError::InvalidLogGroup => {
            write!(fmt, "Invalid Log Group")
        }
        LoggerError::InvalidLogStream => {
            write!(fmt, "Invalid Log Stream")
        }
        LoggerError::InvalidLogEvent => {
            write!(fmt, "Invalid Log Event")
        }
        LoggerError::InvalidLogEventBatch => {
            write!(fmt, "Invalid Log Event Batch")
        }
        LoggerError::DescribeLogStreamError(e) => {
            write!(fmt, "DescribeLogStreamError: {:?}", e)
        }
        LoggerError::SendError(e) => {
            write!(fmt, "SendError: {:?}", e)
        }
        #[cfg(feature = "singleton")]
        LoggerError::Poisoned => {
            write!(fmt, "Poisoned")
        }
    }
}

impl From<SendError<LogLevel>> for LoggerError {
    fn from(e: SendError<LogLevel>) -> Self {
        LoggerError::SendError(e)
    }
}

impl From<rusoto_core::RusotoError<rusoto_logs::DescribeLogStreamsError>> for LoggerError {
    fn from(e: rusoto_core::RusotoError<rusoto_logs::DescribeLogStreamsError>) -> Self {
        LoggerError::DescribeLogStreamError(e)
    }
}

impl std::fmt::Display for LoggerError {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        format_logger_error(self, f)
    }
}

impl std::fmt::Debug for LoggerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        format_logger_error(self, f)
    }
}
impl std::error::Error for LoggerError {}

