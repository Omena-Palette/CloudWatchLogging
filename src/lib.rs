use std::{
    default::Default,
    str::FromStr,
    sync::RwLock,
    env
};
#[cfg(feature = "log-batching")]
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
    sync::Arc
};
#[cfg(feature = "log-batching")]
use tokio::{
    sync::Mutex,
};

use rusoto_logs;
use rusoto_logs::{
    CloudWatchLogs,
    CloudWatchLogsClient,
    DescribeLogStreamsRequest,
    InputLogEvent,
    PutLogEventsRequest
};
use rusoto_core::Region;
use lazy_static::lazy_static;

#[cfg(not(feature = "log-batching"))]
lazy_static! {
    static ref LOGGER: RwLock<Option<Logger>> = RwLock::new(None);
}

#[cfg(feature = "log-batching")]
lazy_static! {
    static ref LOGGER: RwLock<Option<Arc<Mutex<LoggerWorker>>>> = RwLock::new(None);
}

/// # A Logger for AWS CloudWatch Logs
///
/// # Example
/// ```
/// use cloudwatch_logging::Logger;
///
/// #[tokio::main]
/// async fn example() {
///     let mut logger = Logger::get(
///         "test-group".to_string(),
///         "test-stream".to_string()
///     ).await;
///
///     logger.info("test message".to_string()).await;
///     logger.error("test error".to_string()).await;
/// }
/// ```
#[cfg(not(feature = "log-batching"))]
#[derive(Clone)]
pub struct Logger {
    client: CloudWatchLogsClient,
    log_group_name: String,
    log_stream_name: String,
    sequence_token: Option<String>,
}

#[cfg(feature = "log-batching")]
struct LoggerWorker {
    client: CloudWatchLogsClient,
    log_group_name: String,
    log_stream_name: String,
    sequence_token: Option<String>,
    log_events: Arc<Mutex<Vec<InputLogEvent>>>,
    last_flush: RwLock<Instant>,
    flush_interval: Duration,
    flush_on_exit: bool,
    flush_on_exit_signal: Option<AtomicBool>,
}

#[cfg(feature = "log-batching")]
impl Clone for LoggerWorker {
    fn clone(&self) -> Self {
        let flush_on_exit_signal = if self.flush_on_exit {
            Some(AtomicBool::new(false))
        } else {
            None
        };
        Self {
            client: self.client.clone(),
            log_group_name: self.log_group_name.clone(),
            log_stream_name: self.log_stream_name.clone(),
            sequence_token: self.sequence_token.clone(),
            log_events: self.log_events.clone(),
            last_flush: RwLock::new(self.last_flush.read().unwrap().clone()),
            flush_interval: self.flush_interval.clone(),
            flush_on_exit: self.flush_on_exit.clone(),
            flush_on_exit_signal,
        }
    }
}

/// # A Logger for AWS CloudWatch Logs
///
/// # Example
/// ```
/// use cloudwatch_logging::Logger;
///
/// #[tokio::main]
/// async fn example() {
///     let mut logger = Logger::get(
///         "test-group".to_string(),
///         "test-stream".to_string()
///     ).await;
///
///     // this does not flush the logs immediately
///     logger.info("test message".to_string()).await;
///     // this flushes the logs immediately
///     logger.error("test error".to_string()).await;
/// }
/// ```
#[cfg(feature = "log-batching")]
#[derive(Clone)]
pub struct Logger (Arc<Mutex<LoggerWorker>>);

enum LogNature {
    Info,
    Error,
    #[cfg(feature = "log-panics")]
    Panic,
}

fn get_message(log_nature: LogNature, message: String) -> String {
    #[cfg(not(feature = "DEBUG"))]
        let prefix = match log_nature {
        LogNature::Info => format!("INFO: {}", message),
        LogNature::Error => format!("ERROR: {}", message),
        #[cfg(feature = "log-panics")]
        LogNature::Panic => format!("PANIC: {}", message),
    };
    #[cfg(feature = "DEBUG")]
        let prefix = match log_nature {
        LogNature::Info => {
            let message = format!("\x1b[33mINFO:\x1b[0m {}", message);
            println!("{}", message);
            "INFO"
        },
        LogNature::Error => {
            let message = format!("\x1b[31mERROR:\x1b[0m {}", message);
            println!("{}", message);
            "ERROR"
        },
        #[cfg(feature = "log-panics")]
        LogNature::Panic => {
            let message = format!("\x1b[31mPANIC:\x1b[0m {}", message);
            println!("{}", message);
            "PANIC"
        },
    };
    format!("[{}] {}", prefix, message)
}

#[cfg(not(feature = "log-batching"))]
impl Logger {
    /// Creates a new logger instance
    pub async fn new(log_group_name: String, log_stream_name: String) -> Self {
        let client = CloudWatchLogsClient::new(Region::from_str(
            &env::var("AWS_REGION").unwrap_or("us-east-1".to_string())).unwrap()
        );
        let mut desc_streams_req: DescribeLogStreamsRequest = Default::default();
        desc_streams_req.log_group_name = log_group_name.to_string();
        let streams_resp = client.describe_log_streams(desc_streams_req).await;
        let log_streams = streams_resp.unwrap().log_streams.unwrap();
        let stream = &log_streams
            .iter()
            .find(|s| s.log_stream_name == Some(log_stream_name.to_string()))
            .unwrap();
        let sequence_token = stream.upload_sequence_token.clone();

        #[cfg(feature = "log-panics")]
            let mut logger = Self {
            client,
            log_group_name,
            log_stream_name,
            sequence_token,
        };

        #[cfg(feature = "log-panics")]
        {
            use std::panic;
            let logger_clone = logger.clone();
            let default_hook = panic::take_hook();
            panic::set_hook(Box::new(move |panic_info| {
                logger_clone.send_log(panic_info.to_string(), LogNature::Panic).await;
                default_hook(panic_info);
                std::process::exit(1);
            }));
        }

        #[cfg(not(feature = "log-panics"))]
            let logger = Self {
            client,
            log_group_name,
            log_stream_name,
            sequence_token,
        };

        logger
    }

    /// Gets a logger instance
    pub async fn get(log_group_name: String, log_stream_name: String) -> Self {
        if let Ok(read_lock) = LOGGER.try_read() {
            if let Some(logger) = read_lock.as_ref() {
                return logger.clone();
            }
        }

        let mut write_lock = LOGGER.write().unwrap();
        if let Some(client) = write_lock.as_ref() {
            return client.clone();
        }

        let client = Self::new(log_group_name, log_stream_name).await;

        *write_lock = Some(client.clone());
        client
    }

    /// Sends a log message
    async fn send_log(&mut self, message: String, nature: LogNature) {
        let message = get_message(nature, message);
        let input_log_event = InputLogEvent {
            message,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };
        let mut put_req: PutLogEventsRequest = Default::default();
        put_req.log_group_name = self.log_group_name.clone();
        put_req.log_stream_name = self.log_stream_name.clone();
        put_req.sequence_token = self.sequence_token.clone();
        put_req.log_events = vec![input_log_event];
        let put_resp = self.client.put_log_events(put_req).await;
        self.sequence_token = put_resp.unwrap().next_sequence_token;
    }

    /// # Send Info Log
    /// For sending an info log message
    pub async fn info(&mut self, message: String) {
        self.send_log(message, LogNature::Info).await;
    }

    /// # Send Error Log
    /// For sending an error log message, nothing special without feature `log-batching`
    pub async fn error(&mut self, message: String) {
        self.send_log(message, LogNature::Error).await;
    }
}

#[cfg(feature = "log-batching")]
impl LoggerWorker {
    /// Creates a new logger worker
    pub async fn new(log_group_name: String, log_stream_name: String, batch_size: usize) -> Arc<Mutex<Self>> {
        let client = CloudWatchLogsClient::new(Region::from_str(
            &env::var("AWS_REGION").unwrap_or("us-east-1".to_string())).unwrap()
        );
        let mut desc_streams_req: DescribeLogStreamsRequest = Default::default();
        desc_streams_req.log_group_name = log_group_name.to_string();
        let streams_resp = client.describe_log_streams(desc_streams_req).await;
        let log_streams = streams_resp.unwrap().log_streams.unwrap();
        let stream = &log_streams
            .iter()
            .find(|s| s.log_stream_name == Some(log_stream_name.to_string()))
            .unwrap();
        let sequence_token = stream.upload_sequence_token.clone();

        let logger = Arc::new(Mutex::new(Self {
            client,
            log_group_name,
            log_stream_name,
            sequence_token,
            log_events: Arc::new(Mutex::new(Vec::with_capacity(batch_size))),
            last_flush: RwLock::new(Instant::now()),
            flush_interval: Duration::from_secs(5),
            flush_on_exit: true,
            flush_on_exit_signal: Some(AtomicBool::new(false)),
        }));

        // Ensure that panics are handled correctly
        let logger_clone = logger.clone();
        let default_panic_handler = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            #[cfg(feature = "log-panics")]
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                logger_clone.lock().await.send_log(
                    panic_info.to_string(), LogNature::Panic
                ).await;
            });

            tokio::runtime::Runtime::new().unwrap().block_on(async {
                logger_clone.lock().await.flush_on_exit_signal.as_ref().unwrap().store(true, Ordering::SeqCst);
                logger_clone.lock().await.flush().await;
            });

            default_panic_handler(panic_info);
            std::process::exit(1);
        }));

        // start batching on a different thread
        logger.lock().await.start_batching().await;

        logger
    }

    /// # Start Batching
    /// This method is used to start the batching of log events.
    async fn start_batching(&mut self) {
        let mut worker = self.clone();
        let (tx, mut rx) = tokio::sync::oneshot::channel();

        tokio::task::spawn(async move {
            let mut interval = tokio::time::interval(worker.flush_interval);
            loop {
                interval.tick().await;
                worker.flush().await;

                // If being killed, flush, kill the singleton and exit
                match rx.try_recv() {
                    Ok(_) | Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                        println!("LOG BATCHING: Received kill signal, flushing and exiting");
                        worker.flush().await;
                        let mut logger = LOGGER.write().unwrap();
                        *logger = None;
                        tx.send(()).unwrap();
                        break;
                    }
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                        continue;
                    }
                }
            }
        });
    }

    /// # Flush Log Events
    /// This method is used to flush all the log events from the logger worker.
    /// It will take all the log events from the logger worker and send them to CloudWatch Logs.
    /// This method is used in the `start_batching` method.
    pub async fn flush(&mut self) {
        if let Some(events) = self.take_log_events() {
            self.send_events(events).await;
        }
    }

    /// # Take Log Events
    /// This method is used to take all the log events from the logger worker. It will return
    /// a vector of `InputLogEvent` if there are any events to take.
    /// If there are no events to take, it will return `None`.
    /// This method is used in the `flush` method.
    fn take_log_events(&self) -> Option<Vec<InputLogEvent>> {
        let mut log_events = match self.log_events.try_lock() {
            Ok(guard) => guard,
            Err(_) => return None, // lock is held by another thread
        };

        if log_events.is_empty() {
            return None;
        }

        let events = log_events.clone();
        log_events.clear();
        drop(log_events);

        Some(events)
    }

    /// # Get LoggerWorker
    /// This method is used to get a logger worker instance. It will create a new one if it
    /// doesn't exist.
    pub async fn get(log_group_name: String, log_stream_name: String) -> Arc<Mutex<Self>> {
        if let Ok(read_lock) = LOGGER.try_read() {
            if let Some(logger) = read_lock.as_ref() {
                return logger.clone();
            }
        }

        let mut write_lock = LOGGER.write().unwrap();
        if let Some(client) = write_lock.as_ref() {
            return client.clone();
        }

        let client = Self::new(log_group_name, log_stream_name, 100).await;

        *write_lock = Some(client.clone());
        client
    }

    /// # Send Events
    /// This method is used to send a batch of log events to CloudWatch Logs.
    async fn send_events(&mut self, events: Vec<InputLogEvent>) {
        let mut put_req: PutLogEventsRequest = Default::default();
        put_req.log_group_name = self.log_group_name.to_string();
        put_req.log_stream_name = self.log_stream_name.to_string();
        put_req.sequence_token = self.sequence_token.clone();
        put_req.log_events = events;
        let put_resp = self.client.put_log_events(put_req).await;
        if let Ok(resp) = put_resp {
            self.sequence_token = resp.next_sequence_token;
        }
    }

    /// # Send Log
    /// This method is used to append a log to the log events.
    /// It will be sent to CloudWatch Logs when the batch size is reached / the
    /// flush interval is reached.
    async fn send_log(&mut self, message: String, nature: LogNature) {
        let message = get_message(nature, message);
        let input_log_event = InputLogEvent {
            message,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };
        let mut log_events = self.log_events.lock().await;
        log_events.push(input_log_event);
        drop(log_events);
    }
}

#[cfg(feature = "log-batching")]
impl Logger {
    /// # Get Logger
    /// This method is used to get a logger. It will create a new logger if it
    /// does not exist.
    pub async fn get(log_group_name: String, log_stream_name: String) -> Self {
        Self(LoggerWorker::get(log_group_name, log_stream_name).await)
    }

    /// # Info Logging
    /// This method is used to log info. It will not automatically flush the logs
    /// and will wait for the next flush interval.
    /// <br>If you want to flush the logs immediately, use the `flush` method.
    pub async fn info(&mut self, message: String) {
        let logger = self.0.clone();

        tokio::spawn(async move {
            let mut logger = logger.lock().await;
            logger.send_log(message, LogNature::Info).await;
        });
    }

    /// # Error Logging
    /// This method is used to log errors. It will automatically flush the logs
    pub async fn error(&mut self, message: String) {
        let logger = self.0.clone();

        tokio::spawn(async move {
            let mut logger = logger.lock().await;
            logger.send_log(message, LogNature::Error).await;
            logger.flush().await;
        });
    }

    /// # Flush
    /// This method is used to flush the logs. It will be called automatically,
    /// but can be called manually.
    pub async fn flush(&mut self) {
        let mut logger = self.0.lock().await;
        logger.flush().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusoto_logs::OutputLogEvent;

    async fn create_test_log_group_and_stream() {
        use rusoto_logs::{CreateLogGroupRequest, CreateLogStreamRequest};
        let client = CloudWatchLogsClient::new(Region::from_str(
            &env::var("AWS_REGION").unwrap_or("us-east-1".to_string())).unwrap()
        );
        let mut create_log_group_req: CreateLogGroupRequest = Default::default();
        create_log_group_req.log_group_name = "test-group".to_string();
        let _create_log_group_resp = client.create_log_group(create_log_group_req).await;

        let mut create_log_stream_req: CreateLogStreamRequest = Default::default();
        create_log_stream_req.log_group_name = "test-group".to_string();
        create_log_stream_req.log_stream_name = "test-stream".to_string();
        let _create_log_stream_resp = client.create_log_stream(create_log_stream_req).await;
    }

    async fn delete_test_requirements() {
        use rusoto_logs::{DeleteLogGroupRequest, DeleteLogStreamRequest};
        let client = CloudWatchLogsClient::new(Region::from_str(
            &env::var("AWS_REGION").unwrap_or("us-east-1".to_string())).unwrap()
        );
        let mut delete_log_stream_req: DeleteLogStreamRequest = Default::default();
        delete_log_stream_req.log_group_name = "test-group".to_string();
        delete_log_stream_req.log_stream_name = "test-stream".to_string();
        let _delete_log_stream_resp = client.delete_log_stream(delete_log_stream_req).await;

        let mut delete_log_group_req: DeleteLogGroupRequest = Default::default();
        delete_log_group_req.log_group_name = "test-group".to_string();
        let _delete_log_group_resp = client.delete_log_group(delete_log_group_req).await;
    }

    async fn get_test_logs_from_cloudwatch() -> Vec<OutputLogEvent> {
        use rusoto_logs::GetLogEventsRequest;
        let client = CloudWatchLogsClient::new(Region::from_str(
            &env::var("AWS_REGION").unwrap_or("us-east-1".to_string())).unwrap()
        );
        let mut get_log_events_req: GetLogEventsRequest = Default::default();
        get_log_events_req.log_group_name = "test-group".to_string();
        get_log_events_req.log_stream_name = "test-stream".to_string();
        let get_log_events_resp =
            client.get_log_events(get_log_events_req).await;
        get_log_events_resp.unwrap().events.unwrap()
    }

    #[tokio::test]
    async fn test_logger() {
        create_test_log_group_and_stream().await;

        let mut logger = Logger::get(
            "test-group".to_string(), "test-stream".to_string()
        ).await;
        logger.info("test message".to_string()).await;
        logger.error("test error".to_string()).await;

        delete_test_requirements().await;
    }

    #[cfg(feature = "log-batching")]
    #[tokio::test]
    async fn test_batching() {
        create_test_log_group_and_stream().await;
        let mut logger = Logger::get(
            "test-group".to_string(), "test-stream".to_string()
        ).await;

        for _ in 0..100 {
            logger.info("test message".to_string()).await;
        }

        println!("sleeping for 1 second... (to allow logs to be sent)");

        tokio::time::sleep(Duration::from_secs(1)).await;

        // logger.error("test error".to_string()).await;

        tokio::time::sleep(Duration::from_secs(1)).await;

        let events = get_test_logs_from_cloudwatch().await;

        delete_test_requirements().await;
    }

    #[cfg(feature = "log-batching")]
    #[tokio::test]
    async fn test_concurrent_loggers() {
        create_test_log_group_and_stream().await;
        let lw = LoggerWorker::new(
            "test-group".to_string(), "test-stream".to_string(), 50
        ).await;

        let mut logger1 = Logger(lw.clone());
        let mut logger2 = Logger(lw.clone());
        let mut logger3 = Logger(lw.clone());

        for _ in 0..100 {
            logger1.info("test message".to_string()).await;
            logger2.info("test message".to_string()).await;
            logger3.info("test message".to_string()).await;
        }

        println!("sleeping for 1 second... (to allow logs to be sent)");

        tokio::time::sleep(Duration::from_secs(1)).await;

        // logger.error("test error".to_string()).await;

        tokio::time::sleep(Duration::from_secs(1)).await;

        let events = get_test_logs_from_cloudwatch().await;
        println!("{}", events.len());

        delete_test_requirements().await;
    }

    #[cfg(feature = "log-batching")]
    #[tokio::test]
    async fn test_existing_for_while_with_infrequent_logs() {
        create_test_log_group_and_stream().await;

        let mut logger1 = Logger::get(
            "test-group".to_string(), "test-stream".to_string()
        ).await;

        for _ in 0..100 {
            logger1.info("test message".to_string()).await;
            tokio::time::sleep(Duration::from_secs(1)).await;
            let get_cur_events = get_test_logs_from_cloudwatch().await;
            println!("{} events", get_cur_events.len());
        }

        delete_test_requirements().await;
    }
}