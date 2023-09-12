pub type Message = String;

pub enum LogLevel {
    Info(Message),
    Error(Message),
    Panic(Message),
    Flush,
}

#[cfg(feature = "DEBUG")]
impl LogLevel {
    pub(crate) fn debug(&self, timestamp: i64) {
        match self {
            LogLevel::Info(e) => println!(
                "\x1b[90m[{:?}]\x1b[0m \x1b[33mINFO:\x1b[0m {}", timestamp, e
            ),
            LogLevel::Error(e) => println!(
                "\x1b[90m[{}]\x1b[0m \x1b[31mERROR:\x1b[0m {}", timestamp, e
            ),
            LogLevel::Panic(e) => println!(
                "\x1b[90m[{}]\x1b[0m \x1b[31mPANIC:\x1b[0m {}", timestamp, e
            ),
            LogLevel::Flush => {}
        }
    }
}