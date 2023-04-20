use cloudwatch_logging::Logger;
use rusoto_logs::OutputLogEvent;
use std::{env, str::FromStr};
use rusoto_core::Region;
use rusoto_logs::CloudWatchLogsClient;
use rusoto_logs::CloudWatchLogs;

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
async fn test_cloudwatch_logging() {
    create_test_log_group_and_stream().await;

    let mut logger = Logger::get(
        "test-group".to_string(),
        "test-stream".to_string(),
    ).await;

    logger.info("test message".to_string()).await;
}
