# CloudWatch Logging SDK for Rust

This is a wrapper around the community made Rusoto SDK for AWS services. 
It provides a simple interface for logging to CloudWatch in an optimal manner, abstracting away the nuances like 
batching, ensuring flushes, logging panics, etc.

## Version 0.2.0

**Breaking Changes**
- Entry point is now `LoggerHandle` instead of `Logger`
- `Logger::get` is now `LoggerHandle::get_or_setup` with the `singleton` feature enabled
- Now takes static string slices instead of owned strings.

**Improvements**
- Performance and reliability improvements

The api is now stable and will not change unless there is a major version bump. Migrating to the new version
requires very little effort, everything remained the same outside the entry point.

## Usage
```rust
use cloudwatch_logging::{
    LoggerHandle, LoggerError, Logger,
    Duration
};

async fn example() -> Result<(), LoggerError> {
    let logger = LoggerHandle::setup(
        "my-log-group",
        "my-log-stream",
        20, // batch size
        Duration::from_secs(5), // flush interval
    ).await?;
    
    logger.info("Hello, world!".to_string()).await?;
    logger.error("Something went wrong!".to_string()).await?;
}
```

**`singleton` Feature**
```rust
use cloudwatch_logging::{
    LoggerHandle, LoggerError, Logger,
    Duration
};

async fn example() -> Result<(), LoggerError> {
    let logger = LoggerHandle::get_or_setup( // will only setup once
        "my-log-group",
        "my-log-stream",
        20, // batch size
        Duration::from_secs(5), // flush interval
    ).await?;
    
    logger.info("Hello, world!".to_string()).await?;
    logger.error("Something went wrong!".to_string()).await?;
}
```

**Logging Panics**

```rust
use cloudwatch_logging::{
    LoggerHandle, LoggerError, Logger,
    Duration
};

async fn example() -> Result<(), LoggerError> {
    let logger = LoggerHandle::setup(
        "my-log-group",
        "my-log-stream",
        20, // batch size
        Duration::from_secs(5), // flush interval
    ).await?;
    
    logger.log_panics(); // future panics will be logged to cloudwatch
}
```

### Notes

**Cloning the Logger** <br>
Very cheap, same cost as cloning a Sender from a mpsc channel. 
