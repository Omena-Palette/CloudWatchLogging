# CloudWatch Logging SDK for Rust

The CloudWatch Logging SDK for Rust provides a simple and efficient way to log to Amazon CloudWatch Logs.

### Features

- Easy setup for CloudWatch logging.
- Automatic batching and non-blocking flushes for optimal performance.
- Seamless panic logging for enhanced reliability.

### Installation

Add the following dependency to your `Cargo.toml` file:

```toml
[dependencies]
cloudwatch-logging = "0.2.3"
```

### Breaking Changes

#### Version 0.2.0

- Entry point is now `LoggerHandle` instead of `Logger`
- `Logger::get` is now `LoggerHandle::get_or_setup` with the `singleton` feature enabled
- Now takes static string slices instead of owned strings.

<sub>**The api is now stable and will not change unless there is a major version bump. Migrating to the new version
requires very little effort, everything remained the same outside the entry point.**</sub>

### Usage
```rust
use cloudwatch_logging::prelude::*;

async fn example() -> Result<(), LoggerError> {
    let logger = LoggerHandle::setup(
        "my-log-group",
        "my-log-stream",
        20, // batch size
        Duration::from_secs(5), // flush interval
    ).await?;
    
    logger.info("Hello, world!".to_string()).await?;
    logger.error("Something went wrong!".to_string()).await
}
```

**`singleton` Feature**
```rust
use cloudwatch_logging::prelude::*;

#[cfg(feature = "singleton")]
async fn example() -> Result<(), LoggerError> {
    let logger = LoggerHandle::get_or_setup( // will only setup once
        "my-log-group",
        "my-log-stream",
        20, // batch size
        Duration::from_secs(5), // flush interval
    ).await?;
    
    logger.info("Hello, world!".to_string()).await?;
    logger.error("Something went wrong!".to_string()).await
}
```

**Logging Panics**

```rust
use cloudwatch_logging::prelude::*;

async fn example() -> Result<(), LoggerError> {
    let logger = LoggerHandle::setup(
        "my-log-group",
        "my-log-stream",
        20, // batch size
        Duration::from_secs(5), // flush interval
    ).await?;
    
    logger.log_panics() // future panics will be logged to cloudwatch
}
```

### License
This project is licensed under the MIT License - see the [LICENSE](https://github.com/Omena-Palette/CloudWatchLogging/blob/main/LICENSE) file for details.

### Acknowledgements
We'd like to acknowledge the incredible work of the Rusoto community for their AWS SDK, their thoughtful implementation
of Smithy, and their dedication to the Rust community. 

### Rusoto
Rusoto is no longer maintained, although it is stable, and widely used in production still. Once the official AWS SDK
is stable, this library will be updated to use it instead.