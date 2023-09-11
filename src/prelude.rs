use std::env;
use std::str::FromStr;
use rusoto_core::Region;
use tokio::sync::mpsc::{channel, Sender, Receiver};
use tokio::time::timeout;
pub use tokio::time::Duration;

use rusoto_logs::{CloudWatchLogs, CloudWatchLogsClient, DescribeLogStreamsRequest, InputLogEvent};

pub use crate::error::LoggerError;
use crate::levels::LogLevel;

#[cfg(feature = "singleton")]
use lazy_static::lazy_static;

#[cfg(feature = "singleton")]
use crate::sync::Lazy;

/// Default AWS region used for logging.
const FALLBACK_REGION: &str = "us-east-1";

/// Represents a logger that can asynchronously log different levels of information.
///
/// `Logger` provides functionalities for logging informational, error, and panic messages.
/// Each log event is represented with a timestamp and the corresponding message.
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

        let log_event = self.get_log_event(log_message);

        // Using `tokio::spawn` to avoid blocking the panic hook.
        tokio::spawn(async move {
            let _ = self.sender.send(LogLevel::Panic(log_event)).await;
        });
    }

    /// All panics after this call will be logged.
    ///
    /// # Returns
    ///
    /// A result indicating success or containing a `LoggerError` on failure.
    pub fn log_panics(&self) -> Result<(), LoggerError> {
        let logger_clone = self.clone();
        std::panic::set_hook(Box::new(move |info| {
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
        let log_event = self.get_log_event(message);
        self.sender.send(LogLevel::Info(log_event)).await?;
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
        let log_event = self.get_log_event(message);
        self.sender.send(LogLevel::Error(log_event)).await?;
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

    /// Constructs a log event with the provided message and the current timestamp.
    ///
    /// This is a helper method that prepares the data for the actual logging action.
    ///
    /// # Arguments
    ///
    /// * `message` - The message to be logged.
    ///
    /// # Returns
    ///
    /// `InputLogEvent` - A struct containing the message and its timestamp.
    #[inline]
    fn get_log_event(&self, message: String) -> InputLogEvent {
        InputLogEvent {
            message,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }
}

#[async_trait::async_trait]
pub trait Setup<T, E> {
    async fn setup(
        log_group_name: &'static str, log_stream_name: &'static str,
        batch_size: usize, interval: Duration
    ) -> Result<T, E>;
}

/// Handle for asynchronously logging to AWS CloudWatch.
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
impl Setup<Logger, LoggerError> for LoggerHandle {
    /// Initializes a new logger and prepares it for sending logs to AWS CloudWatch.
    /// This returns the sender, as the receiver is used internally by the logger.
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

        let (sender, receiver) = channel(batch_size + 20);

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
}

impl LoggerHandle {
    /// Returns an existing logger instance or sets up a new one, using lazy static initialization.
    /// Requires the "singleton" feature to be enabled.
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
    /// A static ref to a result containing a `Logger` on success or a `LoggerError` on failure.
    #[cfg(feature = "singleton")]
    pub async fn get_or_setup(
        log_group_name: &'static str, log_stream_name: &'static str,
        batch_size: usize, interval: Duration
    ) -> Result<Logger, LoggerError> {
        lazy_static! {
            static ref LOGGER: Lazy<LoggerHandle, Logger> = Lazy::default();
        }

        LOGGER.get_or_init(log_group_name, log_stream_name, batch_size, interval).await.cloned()
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
    pub async fn run(&mut self) -> Result<(), LoggerError> {
        let mut log_batch = Vec::with_capacity(self.max_log_count);

        loop {
            match timeout(self.interval, self.receiver.recv()).await {
                Ok(Some(log_level)) => {
                    #[cfg(feature = "DEBUG")]
                    println!("{:?}", log_level);
                    match log_level {
                        LogLevel::Info(event) => {
                            log_batch.push(event);
                            if log_batch.len() >= self.max_log_count {
                                self.send_to_cloudwatch(
                                    core::mem::take(&mut log_batch)
                                ).await?;
                            }
                        },
                        LogLevel::Error(event) | LogLevel::Panic(event) => {
                            // For Error and Panic, send immediately
                            log_batch.push(event);
                            self.send_to_cloudwatch(
                                core::mem::take(&mut log_batch)
                            ).await?;
                        },
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
}


#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rusoto_logs::OutputLogEvent;

    pub(crate) async fn setup() {
        create_test_log_group_and_stream().await;
    }

    pub(crate) async fn teardown() {
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
            ..Default::default()
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
            ..Default::default()
        };

        let _delete_log_stream_resp = client.delete_log_stream(delete_log_stream_req).await;

        let delete_log_group_req: DeleteLogGroupRequest = DeleteLogGroupRequest {
            log_group_name: "test-group".to_string(),
            ..Default::default()
        };
        let _delete_log_group_resp = client.delete_log_group(delete_log_group_req).await;
    }

    async fn get_test_logs_from_cloudwatch() -> Vec<OutputLogEvent> {
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

        get_log_events_resp.unwrap().events.unwrap()
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

        let test_logs = get_test_logs_from_cloudwatch().await;
        println!("{:?}", test_logs);

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
        assert_eq!(test_logs[0].message, Some("test1".to_string()));
        assert_eq!(test_logs[1].message, Some("test2".to_string()));

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
        assert_eq!(test_logs[0].message, Some("test1".to_string()));
        assert_eq!(test_logs[1].message, Some("test2".to_string()));

        teardown().await;
    }
}