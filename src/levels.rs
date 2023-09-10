use rusoto_logs::InputLogEvent;

pub enum LogLevel {
    Info(InputLogEvent),
    Error(InputLogEvent),
    Panic(InputLogEvent),
    Flush,
}

#[cfg(feature = "DEBUG")]
impl std::fmt::Debug for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Info(e) => write!(
                f, "\x1b[90m[{:?}]\x1b[0m \x1b[33mINFO:\x1b[0m {}", e.timestamp, e.message
            ),
            LogLevel::Error(e) => write!(
                f, "\x1b[90m[{:?}]\x1b[0m \x1b[31mERROR:\x1b[0m {}", e.timestamp, e.message
            ),
            LogLevel::Panic(e) => write!(
                f, "\x1b[90m[{:?}]\x1b[0m \x1b[31mPANIC:\x1b[0m {}", e.timestamp, e.message
            ),
            LogLevel::Flush => Ok(())
        }
    }
}