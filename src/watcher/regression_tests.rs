use super::*;
use crate::test_support::{config, MockHttp, TestTask};

fn start(conf: Conf) -> (watch::Sender<bool>, TestTask) {
    let mut sender = Sender::new(conf.clone()).unwrap();
    let mut watcher = Watcher::new(conf).unwrap();
    let (signal, shutdown) = watch::channel(false);
    let task = TestTask::spawn(async move { watcher.run(&mut sender, shutdown).await });
    (signal, task)
}

async fn assert_query_round_trip(query: &str) {
    let mut http = MockHttp::start().await;
    let mut conf = config(http.port);
    conf.query_string = query.into();
    let (_shutdown, _task) = start(conf);
    let request = http.next("Watcher must send its search request").await;
    let body = request.json();
    assert_eq!(
        body["query"]["bool"]["filter"][1]["query_string"]["query"], query,
        "JSON serialization must preserve the original query string"
    );
    request.respond(200, empty_page());
}

#[tokio::test]
async fn simple_query_reaches_search_unchanged() {
    assert_query_round_trip("level:error AND namespace:production").await;
}

#[tokio::test]
async fn quoted_query_reaches_search_unchanged() {
    assert_query_round_trip(r#"message:"connection refused""#).await;
}

#[tokio::test]
async fn backslashes_reach_search_unchanged() {
    assert_query_round_trip(r"path:C:\logs\app").await;
}

#[tokio::test]
async fn multiline_query_reaches_search_unchanged() {
    assert_query_round_trip("level:error\nAND namespace:production").await;
}

fn empty_page() -> String {
    json!({"hits": {"total": {"value": 0}, "hits": []}}).to_string()
}

fn hits(count: usize, timestamp: &str) -> Vec<Value> {
    (0..count).map(|id| json!({
        "_index": "logs", "_id": id.to_string(),
        "_source": {"@timestamp": timestamp, "message": "same error",
            "pod_name": "pod", "namespace": "ns", "container_name": "app", "pod_id": "pod-id"}
    })).collect()
}

fn page(total: usize, hits: &[Value]) -> String {
    json!({"hits": {"total": {"value": total, "relation": "eq"}, "hits": hits}}).to_string()
}

#[tokio::test]
async fn search_fetches_remaining_hits_instead_of_repeating_first_page() {
    let mut http = MockHttp::start().await;
    let mut slack = MockHttp::start().await;
    let mut conf = config(http.port);
    conf.slack = config(slack.port).slack;
    let (shutdown, mut task) = start(conf);
    let first = http.next("the initial page must be requested").await;
    assert_eq!(first.target, "/logs/_search?preference=logalert");
    let first_query = first.json();
    let timestamp = first_query["query"]["bool"]["filter"][0]["range"]["@timestamp"]["gte"]
        .as_str()
        .unwrap();
    // All 75 hits share a timestamp; a timestamp-only cursor would lose 25.
    let hits = hits(75, timestamp);
    first.respond(200, page(75, &hits[..50]));
    let first_delivery = slack.next("the first page must be delivered").await;
    assert_eq!(
        first_delivery.json()["blocks"][0]["fields"][1]["text"],
        "50"
    );
    assert!(tokio::time::timeout(
        Duration::from_millis(50),
        http.next("wait for delivery before paging")
    )
    .await
    .is_err());
    first_delivery.respond(200, "ok");

    let second = http.next("the remaining 25 hits must be requested").await;
    assert_eq!(second.target, "/logs/_search?preference=logalert");
    let second_query = second.json();
    assert_eq!(second_query["from"], 50);
    assert_eq!(
        first_query["query"], second_query["query"],
        "the search window must stay fixed while paging"
    );
    second.respond(200, page(75, &hits[50..]));
    let second_delivery = slack.next("the last 25 hits must also be delivered").await;
    assert_eq!(
        second_delivery.json()["blocks"][0]["fields"][1]["text"],
        "25"
    );
    shutdown.send_replace(true);
    second_delivery.respond(200, "ok");
    task.finish("finish the current delivery and stop").await;
}

#[tokio::test]
async fn shutdown_during_search_request_is_remembered() {
    let mut http = MockHttp::start().await;
    let (shutdown, mut task) = start(config(http.port));
    let in_flight = http.next("hold search while shutdown arrives").await;
    shutdown.send_replace(true);
    in_flight.respond(200, empty_page());
    task.finish("Watcher must stop after the in-flight search finishes")
        .await;
}

async fn assert_invalid_response_preserves_position(status: u16, response: Value) {
    let mut http = MockHttp::start().await;
    let mut watcher = Watcher::new(config(http.port)).unwrap();
    let mut sender = Sender::new(config(http.port)).unwrap();
    let (_signal, mut shutdown) = watch::channel(false);
    let mut task = TestTask::spawn(async move {
        let start = watcher.start_time;
        assert!(watcher.poll(&mut sender, &mut shutdown).await.is_err());
        assert_eq!(watcher.start_time, start);
        assert!(watcher.pending_window.is_some());
    });
    http.next("search request expected")
        .await
        .respond(status, response.to_string());
    task.finish("invalid responses must fail without advancing the window")
        .await;
}

#[tokio::test]
async fn http_error_preserves_search_position() {
    assert_invalid_response_preserves_position(503, json!({"error":"unavailable"})).await;
}

#[tokio::test]
async fn timed_out_search_preserves_search_position() {
    assert_invalid_response_preserves_position(
        200,
        json!({"timed_out":true,"hits":{"total":{"value":0},"hits":[]}}),
    )
    .await;
}

#[tokio::test]
async fn failed_shard_preserves_search_position() {
    assert_invalid_response_preserves_position(
        200,
        json!({"_shards":{"failed":1},"hits":{"total":{"value":0},"hits":[]}}),
    )
    .await;
}

#[tokio::test]
async fn truncated_total_preserves_search_position() {
    assert_invalid_response_preserves_position(
        200,
        json!({"hits":{"total":{"value":10000,"relation":"gte"},"hits":[]}}),
    )
    .await;
}

#[tokio::test]
async fn result_limit_preserves_search_position() {
    assert_invalid_response_preserves_position(
        200,
        json!({"hits":{"total":{"value":10001},"hits":[]}}),
    )
    .await;
}

#[tokio::test]
async fn empty_page_with_outstanding_hits_preserves_search_position() {
    assert_invalid_response_preserves_position(
        200,
        json!({"hits":{"total":{"value":75},"hits":[]}}),
    )
    .await;
}

#[tokio::test]
async fn failed_delivery_replays_window_without_repeating_acknowledged_groups() {
    let mut http = MockHttp::start().await;
    let mut slack = MockHttp::start().await;
    let mut conf = config(http.port);
    conf.slack = config(slack.port).slack;
    let mut watcher = Watcher::new(conf.clone()).unwrap();
    let mut sender = Sender::new(conf).unwrap();
    let (_signal, mut shutdown) = watch::channel(false);
    let mut task = TestTask::spawn(async move {
        let start = watcher.start_time;
        assert!(watcher.poll(&mut sender, &mut shutdown).await.is_err());
        assert_eq!(watcher.start_time, start);
        let window = watcher.pending_window.unwrap();
        assert!(watcher.poll(&mut sender, &mut shutdown).await.unwrap());
        assert!(watcher.pending_window.is_none());
        assert_eq!(watcher.start_time, window.end - OVERLAP);
    });
    let first = http.next("first search").await;
    let first_query = first.json();
    let mut data = hits(2, "2026-09-28T00:00:00Z");
    data[0]["_source"]["message"] = json!("delivered");
    data[1]["_source"]["message"] = json!("retry me");
    first.respond(200, page(2, &data));
    let delivered = slack.next("first group").await;
    assert!(delivered.json()["text"]
        .as_str()
        .unwrap()
        .contains("delivered"));
    delivered.respond(200, "ok");
    slack
        .next("second group fails")
        .await
        .respond(400, "invalid_payload");
    let replay = http.next("the failed window must be retried").await;
    assert_eq!(replay.json(), first_query);
    replay.respond(200, page(2, &data));
    let retry = slack
        .next("only the failed group must be redelivered")
        .await;
    assert!(retry.json()["text"].as_str().unwrap().contains("retry me"));
    retry.respond(200, "ok");
    task.finish("successful replay must advance the window")
        .await;
}

#[tokio::test]
async fn shutdown_deadline_bounds_an_unresponsive_request() {
    let mut http = MockHttp::start().await;
    let mut watcher = Watcher::new(config(http.port)).unwrap();
    let mut sender = Sender::new(config(http.port)).unwrap();
    let (signal, shutdown) = watch::channel(false);
    let mut task = TestTask::spawn(async move {
        let result = crate::run_until_shutdown(
            &mut watcher,
            &mut sender,
            shutdown,
            Duration::from_millis(20),
        )
        .await;
        assert!(result.unwrap_err().contains("shutdown deadline exceeded"));
    });
    let _in_flight = http.next("hold an unresponsive search open").await;
    signal.send_replace(true);
    task.finish("the global deadline must bound shutdown").await;
}

#[tokio::test]
async fn error_in_successful_http_response_preserves_search_position() {
    assert_invalid_response_preserves_position(
        200,
        json!({"error":"search failed","hits":{"total":{"value":0},"hits":[]}}),
    )
    .await;
}
