# CloudWatch Logging

The CloudWatch Logging crate provides a simple and efficient way to log to [Amazon CloudWatch](https://aws.amazon.com/cloudwatch/).

### Features

- Easy setup for CloudWatch logging.
- Automatic batching and non-blocking flushes for optimal performance.
- Seamless panic logging for enhanced reliability.
- Singleton feature for easy access to the logger from anywhere in your application.
- Thread-safe, ensuring consistent logging across multithreaded applications.

### Installation

Add the following dependency to your `Cargo.toml` file:

```toml
[dependencies]
cloudwatch-logging = "1.0.0"
```

### Usage
```rust
use cloudwatch_logging::prelude::*;

async fn example() -> Result<(), LoggerError> {
    let logger = LoggerHandle::setup(
        "test-group",     // log group
        "test-stream",    // log stream
        20,               // batch size
        Duration::from_secs(5),  // flush interval
    ).await?;
    
    logger.info("Hello, world!".to_string()).await?;
    logger.error("Something went wrong!".to_string()).await
}

cloudwatch_logging::__doc_test!(example);
```

**Logging Panics**

```rust
use cloudwatch_logging::prelude::*;

async fn example() -> Result<(), LoggerError> {
    let logger = LoggerHandle::setup(
        "test-group",   // log group
        "test-stream",  // log stream
        20,             // batch size
        Duration::from_secs(5),  // flush interval
    ).await?;
    
    logger.log_panics();  // future panics will be logged to cloudwatch
    
    panic!("This will be logged to cloudwatch!");
    
    Ok(())
}

cloudwatch_logging::__doc_test_panics!(example, "This will be logged to cloudwatch!");
```

**`singleton` Feature**

```rust
use cloudwatch_logging::prelude::*;

#[cfg(feature = "singleton")]
async fn example() -> Result<(), LoggerError> {
    let logger = LoggerHandle::get_or_setup(  // will only setup once
        "test-group",   // log group
        "test-stream",  // log stream
        20,             // batch size
        Duration::from_secs(5),  // flush interval
    ).await?;
    
    logger.info("Hello, world!".to_string()).await?;
    logger.error("Something went wrong!".to_string()).await
}

#[cfg(feature = "singleton")]
cloudwatch_logging::__doc_test!(example);
```

**`singleton` Feature**: initializing with environment variables

```rust
use cloudwatch_logging::prelude::*;
use std::env;

#[cfg(feature = "singleton")]
async fn example() -> Result<(), LoggerError> {
    env::set_var("TEST_GROUP", "test-group");
    env::set_var("TEST_STREAM", "test-stream");
    
    let logger = LoggerHandle::get_or_setup_with_env(
        "TEST_GROUP",   // log group env var
        "TEST_STREAM",  // log stream env var
        20,             // batch size
        Duration::from_secs(5),  // flush interval
    ).await?;
    
    logger.info("Hello, world!".to_string()).await?;
    logger.error("Something went wrong!".to_string()).await
}

#[cfg(feature = "singleton")]
cloudwatch_logging::__doc_test!(example);
```

### Documentation
For more information, please refer to the [current documentation](https://docs.rs/cloudwatch-logging).

### License
This project is licensed under the MIT License - see the [LICENSE](https://github.com/Omena-Palette/CloudWatchLogging/blob/main/LICENSE) file for details.


### Version 1.0.0 (Stable)
`cloudwatch_logging` is officially stable! While new features will be added, the api will remain the same.

### Upcoming Features
- [ ] `log` crate support
- [ ] `tracing` crate support

### Previous Breaking Changes

#### Version 0.2.0

- Entry point is now `LoggerHandle` instead of `Logger`
- `Logger::get` is now `LoggerHandle::get_or_setup` with the `singleton` feature enabled
- Now takes static string slices instead of owned strings.

<sub>**The api is now stable and will not change unless there is a major version bump. Migrating to the new version
requires very little effort, everything remained the same outside the entry point.**</sub>

### Acknowledgements
We'd like to acknowledge the incredible work of the Rusoto community for their AWS SDK, their thoughtful implementation
of Smithy, and their dedication to the Rust community. 

### Rusoto & Official AWS SDK for Rust
Rusoto is no longer maintained, although, it is still appropriate for and used in production environments. Once the
official AWS SDK for Rust is stable, this crate will be updated to use it instead.

### Testing Considerations:
1. **AWS API Calls**: The tests in this library interact directly with the AWS API. Ensure that your AWS credentials are correctly set up before running tests.
2. **AWS Charges**: Some tests are designed to stress test the library, this can yield charges to your AWS account. Before running any tests, please be aware of potential AWS charges. Omena Inc. is not liable for any costs incurred.
3. **Log Groups and Streams**: Tests target a specific log group and stream (referenced in the examples). Any resources created during testing will be automatically cleaned up.
4. **Parallel Execution**: Avoid running tests in parallel, as they all interact with the same log group and stream. Concurrent runs will cause assertions to fail.

**Running Tests**:
- **Standard Tests**:
  ```bash
  cargo test --features "doc_tests" -- --test-threads=1
  ```

- **Including Loom Models**:
  ```bash
  cargo test --features "doc_tests loom" -- --test-threads=1
  ```

- **Stress Tests**:
  ```bash
  cargo test --features "doc_tests im_ok_paying_for_testing" -- --test-threads=1
  ```