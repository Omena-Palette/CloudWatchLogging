use std::env;
use std::str::FromStr;
use std::time::SystemTime;
use rusoto_core::Region;
use tokio::sync::mpsc::{channel, Sender, Receiver};
use tokio::time::timeout;
pub use tokio::time::Duration;

use rusoto_logs::{CloudWatchLogs, CloudWatchLogsClient, DescribeLogStreamsRequest, InputLogEvent};

pub use crate::error::LoggerError;
use crate::levels::LogLevel;

#[cfg(all(feature = "singleton", not(all(test, feature = "loom"))))]
use crate::sync::{Lazy, new_lazy};

macro_rules! into_log_event (
    ($message:expr, $timestamp:expr) => {
        InputLogEvent {
            message: $message,
            timestamp: $timestamp,
        }
    };
);

/// Default AWS region used for logging.
pub const FALLBACK_REGION: &str = "us-east-1";

/// Represents a logger that can asynchronously log different levels of information.
///
/// <br>
///
/// `Logger` provides functionalities for logging informational, error, and panic messages.
/// Each log event is represented with a timestamp and the corresponding message.
///
/// <br>
///
/// **Usage**: You do not create a Logger directly, instead, you either clone it (as cheap as
/// incrementing an atomic) or use the `singleton` feature to get a static reference to a logger.
/// All `Logger` instances are created by the [`LoggerHandle`] which is a background
/// process that sends logs to AWS CloudWatch. It is best to not have multiple `LoggerHandle`
/// instances running at the same time as the `Logger` is thread safe so you'd only be introducing
/// unnecessary overhead.
///
/// ## Example
///
/// ```
/// use cloudwatch_logging::prelude::*;
///
/// async fn example() -> Result<(), LoggerError> {
///     let logger = LoggerHandle::setup(
///         "test-group", "test-stream", 2, Duration::from_secs(1)
///     ).await?;
///
///     logger.info("test".to_string()).await?;
///     logger.flush().await
/// }
///
/// cloudwatch_logging::__doc_test!(example);
/// ```
///
/// **Singleton Feature**
/// ```
/// use cloudwatch_logging::prelude::*;
///
/// #[cfg(feature = "singleton")]
/// async fn example() -> Result<(), LoggerError> {
///     let logger = LoggerHandle::get_or_setup(
///         "test-group", "test-stream", 2, Duration::from_secs(1)
///     ).await?;
///
///     logger.info("test".to_string()).await?;
///     logger.flush().await
/// }
///
/// #[cfg(feature = "singleton")]
/// cloudwatch_logging::__doc_test!(example);
/// ```
/// <br>
///
/// <sub>**Cloning this is as cheap as incrementing an atomic.**</sub>
#[derive(Clone, Debug)]
pub struct Logger {
    sender: Sender<LogLevel>,
}

impl Logger {
    /// Constructs a new instance of `Logger`.
    ///
    /// # Parameters
    ///
    /// - `sender`: The sender side of an asynchronous channel used for sending log events.
    fn new(sender: Sender<LogLevel>) -> Self {
        Self { sender }
    }

    /// Asynchronously logs a panic event.
    ///
    /// This method should ideally be used as part of a panic hook to capture and log panic
    /// messages.
    /// <br> <br>
    /// **Note**: If you're just looking to log panics, consider using the [`log_panics`](Logger::log_panics)
    /// method instead.
    ///
    /// # Arguments
    ///
    /// * `info` - Information about the panic event.
    pub fn send_panic(self, info: &std::panic::PanicInfo<'_>) {
        let payload = match info.payload().downcast_ref::<&str>() {
            Some(s) => s.to_string(),
            None => "Panic with unknown payload".to_string(),
        };

        let location = info.location().unwrap_or_else(|| std::panic::Location::caller());
        let log_message = format!("PANIC at '{}': {}", location, payload);

        // Using `tokio::spawn` to avoid blocking the panic hook.
        tokio::spawn(async move {
            let _ = self.sender.send(LogLevel::Panic(log_message)).await;
        });
    }

    /// All panics after this call will be logged.
    ///
    /// # Returns
    ///
    /// A result indicating success or containing a `LoggerError` on failure.
    ///
    /// # Example
    /// ```
    /// use cloudwatch_logging::prelude::*;
    ///
    /// async fn example() -> Result<(), LoggerError> {
    ///     let logger = LoggerHandle::setup(
    ///         "test-group", "test-stream", 2, Duration::from_secs(1)
    ///     ).await?;
    ///
    ///     logger.log_panics()?;
    ///     panic!("test"); // sent to cloudwatch
    ///
    ///     Ok(())
    /// }
    ///
    /// cloudwatch_logging::__doc_test_panics!(example, "test");
    /// ```
    pub fn log_panics(&self) -> Result<(), LoggerError> {
        let logger_clone = self.clone();
        std::panic::set_hook(Box::new(move |info| {
            // double clone, as send panic takes ownership. Have to think about edge cases, albeit
            // absurdly rare edge cases. Cloning the logger is also cheap, so this is fine,
            // especially given the fact your program is panicking.
            // You have bigger things to worry about than incrementing an atomic.
            logger_clone.clone().send_panic(info);
        }));
        Ok(())
    }

    /// Logs an informational message.
    ///
    /// # Arguments
    ///
    /// * `message` - The message to be logged.
    ///
    /// # Returns
    ///
    /// A result indicating success or containing a `LoggerError` on failure.
    pub async fn info(&self, message: String) -> Result<(), LoggerError> {
        self.sender.send(LogLevel::Info(message)).await?;
        Ok(())
    }

    /// Logs an error message.
    ///
    /// # Arguments
    ///
    /// * `message` - The message to be logged.
    ///
    /// # Returns
    ///
    /// A result indicating success or containing a `LoggerError` on failure.
    pub async fn error(&self, message: String) -> Result<(), LoggerError> {
        self.sender.send(LogLevel::Error(message)).await?;
        Ok(())
    }

    /// Signals the logger to flush any buffered log messages.
    ///
    /// This is particularly useful to ensure that all messages are sent before shutting down or
    /// transitioning to a different logging behavior.
    ///
    /// # Returns
    ///
    /// A result indicating success or containing a `LoggerError` on failure.
    pub async fn flush(&self) -> Result<(), LoggerError> {
        self.sender.send(LogLevel::Flush).await?;
        Ok(())
    }
}

macro_rules! setup_doc {
    () => {
        concat!(
            " Initializes a new logger and prepares it for sending logs to AWS CloudWatch.\n",
            " This returns the sender, as the receiver is used internally by the logger.\n",
            "\n",
            " # Arguments\n",
            "\n",
            " * `log_group_name` - Name of the AWS CloudWatch Logs group.\n",
            " * `log_stream_name` - Name of the AWS CloudWatch Logs stream.\n",
            " * `batch_size` - Maximum number of log entries to keep before sending a batch to CloudWatch.\n",
            " * `interval` - Time duration to wait between log sends.\n",
            "\n",
            " # Returns\n",
            "\n",
            " A result containing a `Logger` on success or a `LoggerError` on failure. \n",
        )
    }
}

#[cfg(feature = "singleton")]
macro_rules! setup_from_env_doc {
    () => {
        concat!(
            " # Safety\n",
            "\n",
            " This function intentionally leaks the input strings.\n",
            "\n",
            " It's designed to be invoked exclusively by the [`LoggerHandle::get_or_setup_with_env`] method. Specifically, this function\n",
            " isn't directly invoked; instead, it's encapsulated within the private `sync::Lazy<H, T>` struct, ensuring a single execution.\n",
            " This guarantee is validated with `loom` and analyzed using `valgrind`.\n",
            "\n",
            " While this doesn't result in Undefined Behavior (UB), it's marked as `unsafe` due to the memory leak.\n"
        )
    }
}

#[async_trait::async_trait]
pub trait SetupLogger<T, E> {
    #[doc = "<sub>(this is only implemented for and is the same documentation as [LoggerHandle](LoggerHandle)'s implementation)</sub><br><br>"]
    #[doc = setup_doc!()]
    async fn setup(
        log_group_name: &'static str, log_stream_name: &'static str,
        batch_size: usize, interval: Duration
    ) -> Result<T, E>;

    #[cfg(feature = "singleton")]
    #[doc = "<sub>(this is only implemented for and is the same documentation as [LoggerHandle](LoggerHandle)'s implementation)</sub><br><br>"]
    #[doc = setup_from_env_doc!()]
    #[cfg_attr(docsrs, doc(cfg(feature = "singleton")))]
    async unsafe fn setup_with_env(
        log_group_env_name: &'static str, log_stream_env_name: &'static str,
        batch_size: usize, interval: Duration
    ) -> Result<T, E>;
}

/// Handle for asynchronously logging to AWS CloudWatch.
///
/// <br>
///
/// The `LoggerHandle` is always the initial point of contact for logging to AWS CloudWatch with
/// this crate. It is a background process which receives messages from the [`Logger`](Logger)
/// instances and sends them to AWS CloudWatch. When you call the [`setup`](LoggerHandle::setup)
/// (or with `singleton` feature enabled,
#[cfg_attr(feature = "singleton", doc = "[`get_or_setup`](LoggerHandle::get_or_setup)")]
#[cfg_attr(not(feature = "singleton"), doc = "`get_or_setup`")]
/// method),
/// it returns a [`Logger`](Logger) instance which you can use to log messages. This is cheaply
/// cloneable and can be used across threads.
///
/// <br>
///
/// # Example
/// ```
/// use cloudwatch_logging::prelude::*;
///
/// async fn example() -> Result<(), LoggerError> {
///     let logger = LoggerHandle::setup(
///         "test-group",          // log group name
///         "test-stream",         // log stream name
///         2,                     // batch size
///         Duration::from_secs(1) // flush interval
///     ).await?;
///
///     logger.info("test".to_string()).await?;
///     logger.flush().await
/// }
///
/// cloudwatch_logging::__doc_test!(example);
/// ```
pub struct LoggerHandle {
    receiver: Receiver<LogLevel>,
    client: CloudWatchLogsClient,
    sequence_token: Option<String>,
    log_group_name: &'static str,
    log_stream_name: &'static str,
    max_log_count: usize,
    interval: Duration,
}

#[async_trait::async_trait]
impl SetupLogger<Logger, LoggerError> for LoggerHandle {
    #[doc = setup_doc!()]
    async fn setup(
        log_group_name: &'static str, log_stream_name: &'static str,
        batch_size: usize, interval: Duration
    ) -> Result<Logger, LoggerError> {
        let client = CloudWatchLogsClient::new(
            match env::var("AWS_REGION") {
                Ok(region) => Region::from_str(region.as_str()),
                Err(_) => Region::from_str(FALLBACK_REGION)
            }.map_err(|_| LoggerError::InvalidRegion)?
        );

        let (sender, receiver) = channel(batch_size);

        let sequence_token = Self::get_sequence_token(
            &client,
            log_group_name.to_string(),
            log_stream_name.to_string()
        ).await?;

        let mut myself = Self {
            receiver,
            client,
            sequence_token,
            log_group_name,
            log_stream_name,
            interval,
            max_log_count: batch_size,
        };

        tokio::spawn(async move {
            let _ = myself.run().await;
        });

        Ok(Logger::new(sender))
    }

    #[cfg(feature = "singleton")]
    #[doc = setup_from_env_doc!()]
    #[cfg_attr(docsrs, doc(cfg(feature = "singleton")))]
    async unsafe fn setup_with_env(
        log_group_env_name: &'static str, log_stream_env_name: &'static str,
        batch_size: usize, interval: Duration
    ) -> Result<Logger, LoggerError> {
        let log_group_name = env::var(log_group_env_name)
            .map_err(|_| LoggerError::InvalidLogGroup)?;

        let log_stream_name = env::var(log_stream_env_name)
            .map_err(|_| LoggerError::InvalidLogStream)?;

        let log_group_name: &'static str = Box::leak(log_group_name.into_boxed_str());
        let log_stream_name: &'static str = Box::leak(log_stream_name.into_boxed_str());

        Self::setup(log_group_name, log_stream_name, batch_size, interval).await
    }
}

impl LoggerHandle {
    /// Returns an existing logger instance, only ever setting up the background process once.
    /// This is thread safe.
    ///
    /// # Arguments
    ///
    /// * `log_group_name` - Name of the AWS CloudWatch Logs group.
    /// * `log_stream_name` - Name of the AWS CloudWatch Logs stream.
    /// * `batch_size` - Maximum number of log entries to keep before sending a batch to CloudWatch.
    /// * `interval` - Time duration to wait between log sends.
    ///
    /// # Returns
    ///
    /// A result containing a `Logger` on success or a `LoggerError` on failure.
    ///
    /// # Example
    /// ```
    /// use cloudwatch_logging::prelude::*;
    ///
    /// async fn example() -> Result<(), LoggerError> {
    ///     let logger = LoggerHandle::get_or_setup(
    ///         "test-group", "test-stream", 2, Duration::from_secs(1)
    ///     ).await?;
    ///
    ///     logger.info("test".to_string()).await?;
    ///     logger.flush().await?;
    ///
    ///     Ok(())
    /// }
    ///
    /// cloudwatch_logging::__doc_test!(example);
    /// ```
    #[cfg_attr(docsrs, doc(cfg(feature = "singleton")))]
    #[cfg(all(feature = "singleton", not(all(test, feature = "loom"))))]
    pub async fn get_or_setup(
        log_group_name: &'static str, log_stream_name: &'static str,
        batch_size: usize, interval: Duration
    ) -> Result<Logger, LoggerError> {
        static LOGGER: Lazy<LoggerHandle, Logger> = new_lazy();
        LOGGER.get_or_init::<false>(
            log_group_name, log_stream_name, batch_size, interval
        ).await.cloned()
    }

    /// [get_or_setup](LoggerHandle::get_or_setup) but with environment variable names for the
    /// log group and stream names.
    ///
    /// <br>
    ///
    /// This leaks the input strings, as this is a static background process, these values must
    /// live until the end of the program.
    ///
    /// # Arguments
    ///
    /// * `log_group_env_name` - Key for the environment variable containing the AWS CloudWatch Log
    ///                          group name.
    /// * `log_stream_env_name` - Key for the environment variable containing the AWS CloudWatch Log
    ///                           stream name.
    /// * `batch_size` - Maximum number of log entries to keep before sending a batch to CloudWatch.
    /// * `interval` - Time duration to wait between log sends.
    ///
    /// # Returns
    ///
    /// A result containing a `Logger` on success or a `LoggerError` on failure.
    ///
    /// # Example
    /// ```
    /// use cloudwatch_logging::prelude::*;
    /// use std::env;
    ///
    /// async fn example() -> Result<(), LoggerError> {
    ///     env::set_var("TEST_GROUP", "test-group");
    ///     env::set_var("TEST_STREAM", "test-stream");
    ///
    ///     let logger = LoggerHandle::get_or_setup_with_env(
    ///         "TEST_GROUP", "TEST_STREAM", 2, Duration::from_secs(1)
    ///     ).await?;
    ///
    ///     logger.info("test".to_string()).await?;
    ///     logger.flush().await?;
    ///
    ///     Ok(())
    /// }
    ///
    /// cloudwatch_logging::__doc_test!(example);
    /// ```
    #[cfg(all(feature = "singleton", not(all(test, feature = "loom"))))]
    #[cfg_attr(docsrs, doc(cfg(feature = "singleton")))]
    pub async fn get_or_setup_with_env(
        log_group_env_name: &'static str, log_stream_env_name: &'static str,
        batch_size: usize, interval: Duration
    ) -> Result<Logger, LoggerError> {
        static LOGGER: Lazy<LoggerHandle, Logger> = new_lazy();
        LOGGER.get_or_init::<true>(
            log_group_env_name, log_stream_env_name,
            batch_size, interval
        )
            .await
            .cloned()
    }

    /// Retrieves the sequence token for the specified log stream.
    ///
    /// # Arguments
    ///
    /// * `client` - AWS CloudWatch Logs client.
    /// * `log_group_name` - Name of the AWS CloudWatch Logs group.
    /// * `log_stream_name` - Name of the AWS CloudWatch Logs stream.
    ///
    /// # Returns
    ///
    /// A result containing the sequence token on success or a `LoggerError` on failure.
    pub async fn get_sequence_token(
        client: &CloudWatchLogsClient,
        log_group_name: String,
        log_stream_name: String
    ) -> Result<Option<String>, LoggerError> {
        let req = DescribeLogStreamsRequest {
            log_group_name,
            log_stream_name_prefix: Some(log_stream_name),
            ..Default::default()
        };

        let res = client.describe_log_streams(req).await?;

        let log_stream = res.log_streams
            .ok_or(LoggerError::InvalidLogStream)?
            .into_iter()
            .next()
            .ok_or(LoggerError::InvalidLogStream)?;

        Ok(log_stream.upload_sequence_token)
    }

    /// Batches log messages and sends them to CloudWatch Logs at specified intervals or
    /// when the batch size is reached.
    async fn run(&mut self) -> Result<(), LoggerError> {
        let mut log_batch = Vec::with_capacity(self.max_log_count);

        loop {
            match timeout(self.interval, self.receiver.recv()).await {
                Ok(Some(log_level)) => {
                    let timestamp = SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)?
                        .as_millis() as i64;

                    #[cfg(feature = "DEBUG")]
                    log_level.debug(timestamp);

                    match log_level {
                        LogLevel::Info(event) => {
                            log_batch.push(into_log_event!(
                                Self::prepend_to_string("INFO: ", event), timestamp)
                            );
                            if log_batch.len() >= self.max_log_count {
                                self.send_to_cloudwatch(
                                    core::mem::take(&mut log_batch)
                                ).await?;
                            }
                        },
                        // For Error and Panic, flush the logs immediately
                        LogLevel::Error(event) => {
                            log_batch.push(into_log_event!(
                                Self::prepend_to_string("ERROR: ", event), timestamp
                            ));
                            self.send_to_cloudwatch(
                                core::mem::take(&mut log_batch)
                            ).await?;
                        },
                        LogLevel::Panic(message) => {
                            log_batch.push(into_log_event!(
                                message, timestamp
                            ));
                            self.send_to_cloudwatch(
                                core::mem::take(&mut log_batch)
                            ).await?;
                        }
                        LogLevel::Flush => {
                            // Flush the logs
                            if !log_batch.is_empty() {
                                self.send_to_cloudwatch(
                                    core::mem::take(&mut log_batch)
                                ).await?;
                            }
                        }
                    }
                },
                Ok(None) => {
                    // The channel was closed
                    break;
                },
                Err(_) => {
                    // Timeout elapsed, so we send any accumulated logs
                    if !log_batch.is_empty() {
                        self.send_to_cloudwatch(
                            core::mem::take(&mut log_batch)
                        ).await?;
                    }
                }
            }
        }

        // Send remaining logs if any (this caters for accumulated Info logs)
        if !log_batch.is_empty() {
            self.send_to_cloudwatch(log_batch).await?;
        }

        Ok(())
    }

    /// Sends the specified batch of log events to AWS CloudWatch.
    ///
    /// # Arguments
    ///
    /// * `log_batch` - A vector of log events to be sent.
    ///
    /// # Returns
    ///
    /// A result indicating success or containing a `LoggerError` on failure.
    async fn send_to_cloudwatch(
        &mut self, log_batch: Vec<InputLogEvent>
    ) -> Result<(), LoggerError> {
        let sequence_token = core::mem::take(&mut self.sequence_token);

        let put_req = rusoto_logs::PutLogEventsRequest {
            log_events: log_batch,
            log_group_name: self.log_group_name.to_string(),
            log_stream_name: self.log_stream_name.to_string(),
            sequence_token,
        };

        let put_res = self.client.put_log_events(put_req).await
            .map_err(|_| LoggerError::InvalidLogStream)?;

        self.sequence_token = put_res.next_sequence_token;

        Ok(())
    }

    fn prepend_to_string(base: &'static str, mut owned_string: String) -> String {
        let additional_capacity = base.len();
        owned_string.reserve(additional_capacity);
        owned_string.insert_str(0, base);
        owned_string
    }
}

#[cfg(any(feature = "doc_tests", test))]
pub mod __tests {
    use super::*;
    use rusoto_logs::OutputLogEvent;

    #[allow(dead_code)]
    #[macro_export]
    macro_rules! __help_msg {
        () => {
            "If you aren't already, you must be running with test thread count set to 1. \
            This isn't because of any limitation in the library, but due to the tests actually \
            hitting cloudwatch, and the same stream, so assertions will fail, teardowns will \
            overlap, and things will break."
        };
    }

    pub async fn setup() {
        create_test_log_group_and_stream().await;
    }

    pub async fn teardown() {
        delete_test_requirements().await;
    }
    async fn create_test_log_group_and_stream() {
        use rusoto_logs::{CreateLogGroupRequest, CreateLogStreamRequest};
        let client = CloudWatchLogsClient::new(Region::from_str(
            &env::var("AWS_REGION").unwrap_or("us-east-1".to_string())).unwrap()
        );
        let create_log_group_req: CreateLogGroupRequest = CreateLogGroupRequest {
            log_group_name: "test-group".to_string(),
            ..Default::default()
        };

        let _create_log_group_resp = client.create_log_group(create_log_group_req).await;

        let create_log_stream_req: CreateLogStreamRequest = CreateLogStreamRequest {
            log_group_name: "test-group".to_string(),
            log_stream_name: "test-stream".to_string(),
        };

        let _create_log_stream_resp = client.create_log_stream(create_log_stream_req).await;
    }

    async fn delete_test_requirements() {
        use rusoto_logs::{DeleteLogGroupRequest, DeleteLogStreamRequest};
        let client = CloudWatchLogsClient::new(Region::from_str(
            &env::var("AWS_REGION").unwrap_or("us-east-1".to_string())).unwrap()
        );
        let delete_log_stream_req: DeleteLogStreamRequest = DeleteLogStreamRequest {
            log_group_name: "test-group".to_string(),
            log_stream_name: "test-stream".to_string(),
        };

        let _delete_log_stream_resp = client.delete_log_stream(delete_log_stream_req).await;

        let delete_log_group_req: DeleteLogGroupRequest = DeleteLogGroupRequest {
            log_group_name: "test-group".to_string(),
        };
        let _delete_log_group_resp = client.delete_log_group(delete_log_group_req).await;
    }

    #[allow(dead_code)]
    pub async fn get_test_logs_from_cloudwatch() -> Vec<OutputLogEvent> {
        use rusoto_logs::GetLogEventsRequest;
        let client = CloudWatchLogsClient::new(Region::from_str(
            &env::var("AWS_REGION").unwrap_or("us-east-1".to_string())).unwrap()
        );

        let get_log_events_req: GetLogEventsRequest = GetLogEventsRequest {
            log_group_name: "test-group".to_string(),
            log_stream_name: "test-stream".to_string(),
            ..Default::default()
        };
        let get_log_events_resp =
            client.get_log_events(get_log_events_req).await;

        get_log_events_resp.expect(__help_msg!()).events.expect(__help_msg!())
    }

    #[tokio::test]
    async fn run_logger() {
        setup().await;
        let logger = LoggerHandle::setup(
            "test-group", "test-stream", 1, Duration::from_secs(1)
        ).await.unwrap();

        logger.info("test".to_string()).await.unwrap();
        logger.flush().await.unwrap();

        tokio::time::sleep(Duration::from_secs(1)).await;

        let _test_logs = get_test_logs_from_cloudwatch().await;

        teardown().await;
    }

    #[tokio::test]
    async fn test_logger_batching() {
        setup().await;
        let logger = LoggerHandle::setup(
            "test-group", "test-stream", 2,
            Duration::from_secs(1)
        ).await.unwrap();

        logger.info("test1".to_string()).await.unwrap();
        logger.info("test2".to_string()).await.unwrap();
        logger.flush().await.unwrap();

        tokio::time::sleep(Duration::from_secs(1)).await;

        let test_logs = get_test_logs_from_cloudwatch().await;

        let first_log = test_logs.get(0).expect(__help_msg!());
        let second_log = test_logs.get(1).expect(__help_msg!());

        assert_eq!(first_log.message, Some("INFO: test1".to_string()));
        assert_eq!(second_log.message, Some("INFO: test2".to_string()));

        teardown().await;
    }

    #[tokio::test]
    async fn test_logger_batching_timeout() {
        setup().await;
        let logger = LoggerHandle::setup(
            "test-group", "test-stream", 2, Duration::from_secs(1)
        ).await.unwrap();

        logger.info("test1".to_string()).await.unwrap();
        logger.info("test2".to_string()).await.unwrap();
        logger.flush().await.unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;

        let test_logs = get_test_logs_from_cloudwatch().await;
        let first_msg = test_logs.get(0).expect(__help_msg!());
        let second_msg = test_logs.get(1).expect(__help_msg!());

        assert_eq!(first_msg.message, Some("INFO: test1".to_string()));
        assert_eq!(second_msg.message, Some("INFO: test2".to_string()));

        teardown().await;
    }

    #[tokio::test]
    async fn test_not_enough_space() {
        setup().await;
        let logger = LoggerHandle::setup(
            "test-group", "test-stream", 2, Duration::from_secs(1)
        ).await.unwrap();

        logger.info("test1".to_string()).await.unwrap();
        logger.info("test2".to_string()).await.unwrap();
        logger.info("test3".to_string()).await.unwrap();
        logger.flush().await.unwrap();

        tokio::time::sleep(Duration::from_secs(1)).await;

        let test_logs = get_test_logs_from_cloudwatch().await;
        let first_msg = test_logs.get(0).expect(__help_msg!());
        let second_msg = test_logs.get(1).expect(__help_msg!());
        let third_msg = test_logs.get(2).expect(__help_msg!());

        assert_eq!(first_msg.message, Some("INFO: test1".to_string()));
        assert_eq!(second_msg.message, Some("INFO: test2".to_string()));
        assert_eq!(third_msg.message, Some("INFO: test3".to_string()));

        teardown().await;
    }

    #[cfg(feature = "im_ok_paying_for_testing")]
    #[allow(dead_code)]
    const SPAM_THREAD_COUNT: usize = 10;
    #[cfg(feature = "im_ok_paying_for_testing")]
    #[allow(dead_code)]
    const SPAM_LOG_COUNT: usize = 100;

    #[cfg(feature = "im_ok_paying_for_testing")]
    #[tokio::test]
    async fn spam_not_enough_space() {
        setup().await;
        let logger = LoggerHandle::setup(
            "test-group", "test-stream", 2, Duration::from_secs(1)
        ).await.unwrap();

        let mut handles = Vec::with_capacity(SPAM_THREAD_COUNT);

        for _ in 0..SPAM_THREAD_COUNT {
            handles.push(spam_logger(&logger));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        logger.flush().await.unwrap();

        tokio::time::sleep(Duration::from_secs(1)).await;

        let test_logs = get_test_logs_from_cloudwatch().await;

        assert_eq!(test_logs.len(), SPAM_THREAD_COUNT * SPAM_LOG_COUNT);

        teardown().await;
    }

    #[cfg(feature = "im_ok_paying_for_testing")]
    #[allow(dead_code)]
    fn spam_logger(logger: &Logger) -> tokio::task::JoinHandle<()> {
        let logger = logger.clone();

        tokio::spawn(async move {
            for _ in 0..SPAM_LOG_COUNT {
                logger.info("test".to_string()).await.unwrap();
            }
        })
    }

    #[allow(dead_code)]
    #[macro_export]
    macro_rules! __doc_test {
        ($test_fn:expr) => {
            #[cfg(feature = "doc_tests")]
            tokio_test::block_on(async {
                __tests::setup().await;
                $test_fn().await.unwrap();
                __tests::teardown().await;
            });
        };
    }

    #[allow(dead_code)]
    #[macro_export]
    macro_rules! __doc_test_panics {
        ($fn_ident:ident, $panic_msg:expr) => {
            #[cfg(feature = "doc_tests")]
            tokio_test::block_on(async {
                // Setup
                ::cloudwatch_logging::prelude::__tests::setup().await;

                // Run the example function in a separate Tokio task
                let result = tokio::task::spawn(async { $fn_ident().await }).await;

                // wait a bit
                tokio::time::sleep(Duration::from_secs(1)).await;

                assert!(result.is_err());

                // Check that the message was logged
                let test_logs = ::cloudwatch_logging::prelude::__tests::get_test_logs_from_cloudwatch().await;

                let log_msg = test_logs.get(0).expect(::cloudwatch_logging::__help_msg!());
                let log_msg = log_msg.message.clone().unwrap();
                let log_msg = log_msg.rsplit(':').next().unwrap().trim();

                assert_eq!(log_msg, $panic_msg);

                // Teardown
                ::cloudwatch_logging::prelude::__tests::teardown().await;
            });
        };
    }
}