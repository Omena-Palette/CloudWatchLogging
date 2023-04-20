# CloudWatch Logging SDK for Rust

This is a wrapper around the community made Rusoto SDK for AWS services. 
It provides a simple interface for logging to CloudWatch in an optimal manner, abstracting away the nuances like 
batching, ensuring flushes, logging panics, etc.

## Usage
```rust
use cloudwatch_logging::Logger;

async fn example() {
    let mut logger = Logger::get("my-log-group", "my-log-stream").await;
    logger.info("Hello, world!".to_string()).await;
    logger.error("Something went wrong!".to_string()).await;
}
```

## Features
- [x] log-batching: Batches logs together to reduce the number of API calls
- [x] log-panics: Logs panics to CloudWatch overwriting the default panic handler
- [ ] DEBUG: Enables logging to stdout