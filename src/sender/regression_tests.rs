use super::*;
use crate::test_support::{config, event, MockHttp, TestTask};

fn sender(port: u16) -> Sender {
    let mut sender = Sender::new(config(port)).unwrap();
    sender.retry_delay = Duration::from_millis(5);
    sender
}

async fn assert_failed_delivery_can_be_retried(status: Option<u16>) {
    let mut http = MockHttp::start().await;
    let mut sender = sender(http.port);
    let (_signal, mut shutdown) = watch::channel(false);
    let mut task = TestTask::spawn(async move {
        assert!(sender
            .send(vec![event("retry-id", "retry me")], &mut shutdown)
            .await
            .unwrap());
        assert_eq!(sender.sent.len(), 1);
    });
    let first = http.next("initial delivery must reach Slack").await;
    let payload = first.json();
    if let Some(status) = status {
        first.respond(status, "unavailable");
    } else {
        drop(first);
    }
    let retry = http.next("an unsuccessful delivery must be retried").await;
    assert_eq!(retry.json(), payload);
    retry.respond(200, "ok");
    task.finish("the successful retry must complete delivery")
        .await;
}

#[tokio::test]
async fn server_error_does_not_mark_event_as_delivered() {
    assert_failed_delivery_can_be_retried(Some(500)).await;
}

#[tokio::test]
async fn rate_limit_does_not_mark_event_as_delivered() {
    assert_failed_delivery_can_be_retried(Some(429)).await;
}

#[tokio::test]
async fn disconnected_request_does_not_mark_event_as_delivered() {
    assert_failed_delivery_can_be_retried(None).await;
}

#[tokio::test]
async fn successful_delivery_is_deduplicated() {
    let mut http = MockHttp::start().await;
    let mut sender = sender(http.port);
    let (_signal, mut shutdown) = watch::channel(false);
    let mut task = TestTask::spawn(async move {
        assert!(sender
            .send(vec![event("delivered", "first message")], &mut shutdown)
            .await
            .unwrap());
        assert!(sender
            .send(vec![event("delivered", "first message")], &mut shutdown)
            .await
            .unwrap());
        assert!(sender
            .send(vec![event("next", "next message")], &mut shutdown)
            .await
            .unwrap());
    });
    http.next("initial delivery must reach Slack")
        .await
        .respond(200, "ok");
    let next = http.next("the next distinct event must be delivered").await;
    assert!(next.json()["text"]
        .as_str()
        .unwrap()
        .contains("next message"));
    next.respond(200, "ok");
    task.finish("both deliveries must complete").await;
}

#[tokio::test]
async fn aggregated_delivery_counts_distinct_events_once() {
    let mut http = MockHttp::start().await;
    let mut sender = sender(http.port);
    let (_signal, mut shutdown) = watch::channel(false);
    let mut task = TestTask::spawn(async move {
        assert!(sender
            .send(
                vec![
                    event("one", "same error"),
                    event("two", "same error"),
                    event("one", "same error")
                ],
                &mut shutdown
            )
            .await
            .unwrap());
        assert_eq!(sender.sent.len(), 2);
        assert!(sender
            .send(
                vec![event("one", "same error"), event("two", "same error")],
                &mut shutdown
            )
            .await
            .unwrap());
        assert!(sender
            .send(vec![event("barrier", "after aggregate")], &mut shutdown)
            .await
            .unwrap());
    });
    let aggregate = http.next("the aggregate must be delivered").await;
    assert_eq!(aggregate.json()["blocks"][0]["fields"][1]["text"], "2");
    aggregate.respond(200, "ok");
    let next = http.next("both aggregated IDs must be deduplicated").await;
    assert!(next.json()["text"]
        .as_str()
        .unwrap()
        .contains("after aggregate"));
    next.respond(200, "ok");
    task.finish("aggregate and following event must complete")
        .await;
}

#[tokio::test]
async fn queue_pressure_does_not_stop_delivery() {
    // The old broadcast queue stopped after 1024 pending batches. Sequential
    // calls now apply backpressure directly, retaining all distinct events.
    let mut http = MockHttp::start().await;
    let mut sender = sender(http.port);
    let (_signal, mut shutdown) = watch::channel(false);
    let mut task = TestTask::spawn(async move {
        assert!(sender
            .send(vec![event("in-flight", "blocked request")], &mut shutdown)
            .await
            .unwrap());
        let burst = (0..1026)
            .map(|id| event(&id.to_string(), "queued message"))
            .collect();
        assert!(sender.send(burst, &mut shutdown).await.unwrap());
        assert!(sender
            .send(vec![event("last", "last message")], &mut shutdown)
            .await
            .unwrap());
        assert_eq!(sender.sent.len(), 1028);
    });
    http.next("hold the first request open")
        .await
        .respond(200, "ok");
    let queued = http.next("the entire burst must be delivered").await;
    assert_eq!(queued.json()["blocks"][0]["fields"][1]["text"], "1026");
    queued.respond(200, "ok");
    let last = http.next("the final event must not be lost").await;
    assert!(last.json()["text"]
        .as_str()
        .unwrap()
        .contains("last message"));
    last.respond(200, "ok");
    task.finish("all events must be acknowledged").await;
}

#[tokio::test]
async fn shutdown_during_http_request_is_remembered() {
    let mut http = MockHttp::start().await;
    let mut sender = sender(http.port);
    let (signal, mut shutdown) = watch::channel(false);
    let mut task = TestTask::spawn(async move {
        assert!(!sender
            .send(
                vec![
                    event("in-flight", "complete before exit"),
                    event("later", "do not start")
                ],
                &mut shutdown
            )
            .await
            .unwrap());
        assert_eq!(
            sender.sent.len(),
            1,
            "finish and acknowledge only the in-flight notification"
        );
    });
    let in_flight = http.next("hold delivery while shutdown arrives").await;
    signal.send_replace(true);
    in_flight.respond(200, "ok");
    task.finish("Sender must stop after the in-flight request finishes")
        .await;
}

#[tokio::test]
async fn exhausted_retries_leave_event_available_for_redelivery() {
    let mut http = MockHttp::start().await;
    let mut sender = sender(http.port);
    let (_signal, mut shutdown) = watch::channel(false);
    let mut task = TestTask::spawn(async move {
        assert!(sender
            .send(vec![event("retry", "error")], &mut shutdown)
            .await
            .is_err());
        assert!(sender.sent.is_empty());
        assert!(sender
            .send(vec![event("retry", "error")], &mut shutdown)
            .await
            .unwrap());
        assert_eq!(sender.sent.len(), 1);
    });
    for _ in 0..MAX_ATTEMPTS {
        http.next("retry budget must be used")
            .await
            .respond(500, "unavailable");
    }
    http.next("a later call must still be able to deliver the event")
        .await
        .respond(200, "ok");
    task.finish("redelivery must succeed").await;
}

#[tokio::test]
async fn permanent_failure_does_not_mark_delivery_or_retry() {
    let mut http = MockHttp::start().await;
    let mut sender = sender(http.port);
    let (_signal, mut shutdown) = watch::channel(false);
    let mut task = TestTask::spawn(async move {
        assert!(sender
            .send(vec![event("invalid", "invalid")], &mut shutdown)
            .await
            .is_err());
        assert!(sender.sent.is_empty());
    });
    http.next("request must reach Slack")
        .await
        .respond(400, "invalid_payload");
    task.finish("a permanent error must return immediately")
        .await;
}

#[tokio::test]
async fn retry_after_is_honored_and_shutdown_interrupts_wait() {
    let mut http = MockHttp::start().await;
    let mut sender = sender(http.port);
    let (signal, mut shutdown) = watch::channel(false);
    let mut task = TestTask::spawn(async move {
        assert!(!sender
            .send(vec![event("rate-limit", "limited")], &mut shutdown)
            .await
            .unwrap());
        assert!(sender.sent.is_empty());
    });
    http.next("request must reach Slack")
        .await
        .respond_with_headers(429, "rate limited", &[("Retry-After", "60")]);
    // Wait beyond the normal retry delay, while still inside Retry-After.
    assert!(
        tokio::time::timeout(Duration::from_millis(50), http.next("no early retry"))
            .await
            .is_err()
    );
    signal.send_replace(true);
    task.finish("shutdown must interrupt Retry-After").await;
}

#[tokio::test]
async fn identical_document_ids_in_different_indices_are_distinct() {
    let mut sender = sender(1);
    sender
        .sent
        .insert(("old-index".into(), "same-id".into()), Instant::now());
    let mut e = event("same-id", "new index");
    e.index = "new-index".into();
    assert_eq!(sender.aggregate(vec![e]).unwrap().len(), 1);
}

#[test]
fn aggregation_does_not_confuse_namespace_separators() {
    let sender = sender(1);
    let mut first = event("one", "error-prod");
    first.meta.namespace = "app".into();
    let mut second = event("two", "error");
    second.meta.namespace = "prod-app".into();
    assert_eq!(sender.aggregate(vec![first, second]).unwrap().len(), 2);
}

#[tokio::test]
async fn retry_after_survives_exhausted_attempts_and_window_replay() {
    let mut http = MockHttp::start().await;
    let mut sender = sender(http.port);
    let (signal, mut shutdown) = watch::channel(false);
    let mut task = TestTask::spawn(async move {
        assert!(sender
            .send(vec![event("limited", "error")], &mut shutdown)
            .await
            .is_err());
        assert!(!sender
            .send(vec![event("limited", "error")], &mut shutdown)
            .await
            .unwrap());
        assert!(sender.sent.is_empty());
    });
    for _ in 1..MAX_ATTEMPTS {
        http.next("transient failure")
            .await
            .respond(500, "unavailable");
    }
    http.next("final attempt is rate limited")
        .await
        .respond_with_headers(429, "limited", &[("Retry-After", "60")]);
    assert!(tokio::time::timeout(
        Duration::from_millis(50),
        http.next("replay must respect cooldown")
    )
    .await
    .is_err());
    signal.send_replace(true);
    task.finish("cooldown remains cancellable across calls")
        .await;
}
