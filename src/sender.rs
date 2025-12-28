use std::collections::{hash_map::Entry, HashMap};
use std::ops::Sub;
use std::sync::Arc;
use std::time::Duration;

use chrono::TimeDelta;
use handlebars::{no_escape, Handlebars};
use log::{debug, error};
use regex::Regex;
use reqwest::Client;
use tokio::sync::{broadcast, Notify, RwLock};
use tokio::time;

use crate::entities::event::Event;
use crate::entities::message::Message;
use crate::entities::slack::Slack;
use crate::Conf;

const SEND_QUEUE_SIZE: usize = 1024;
const CLEANUP_INTERVAL: i64 = 3600;
const EVENTS_SIZE_THRESHOLD: usize = 100000;

const CONTENT_TYPE: &str = "Content-Type";
const JSON_TYPE: &str = "application/json";
const RFC3339_REGEX: &str = r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,9})?Z";

pub struct Sender {
    conf: Conf,
    sender: broadcast::Sender<Vec<Event>>,
    client: Client,
    sent: RwLock<HashMap<String, i64>>,
    regexp: Regex,
}

impl Sender {
    pub fn new(conf: Conf) -> Result<Self, String> {
        let (tx, _) = broadcast::channel(SEND_QUEUE_SIZE);

        let regexp = Regex::new(RFC3339_REGEX)
            .map_err(|e| format!("failed to compile RFC3339 regex: {}", e))?;

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| format!("failed to create HTTP client: {}", e))?;

        Ok(Sender {
            conf,
            sender: tx,
            client,
            sent: RwLock::new(HashMap::new()),
            regexp,
        })
    }

    pub async fn run(&self, notify: Arc<Notify>) {
        let mut handlebars = Handlebars::new();
        handlebars.register_escape_fn(no_escape);

        if let Err(e) =
            handlebars.register_template_string("slack", include_str!("templates/slack.hbs"))
        {
            error!("failed to register slack template: {}", e);
            return;
        }

        let mut rx = self.sender.subscribe();

        let mut ticker = time::interval(Duration::from_secs(CLEANUP_INTERVAL as u64));

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let mut map = self.sent.write().await;

                    if map.len() > EVENTS_SIZE_THRESHOLD {
                        match TimeDelta::try_seconds(CLEANUP_INTERVAL) {
                            Some(delta) => {
                                let cleanup_threshold = chrono::Utc::now().sub(delta).timestamp();
                                let before_count = map.len();
                                map.retain(|_, v| *v > cleanup_threshold);
                                debug!("cleanup done: removed {} entries", before_count - map.len());
                            }
                            None => {
                                error!("invalid cleanup interval: {}", CLEANUP_INTERVAL);
                            }
                        }
                    }
                }
                events = rx.recv() => {
                    let events = match events {
                        Ok(events) => events,
                        Err(e) => {
                            error!("error receiving event: {}", e);
                            break;
                        }
                    };

                    let mut frequency_map : HashMap<String,Message> = HashMap::new();

                    for e in events {
                        let event_id = &e.id;

                        // Check and add atomically
                        let already_sent = {
                            let mut sent_map = self.sent.write().await;
                            if sent_map.contains_key(event_id) {
                                true
                            } else {
                                sent_map.insert(event_id.clone(), chrono::Utc::now().timestamp());
                                false
                            }
                        };

                        if already_sent {
                            continue;
                        }

                        let key = format!("{}-{}", e.message, e.meta.namespace);

                        let key = self.regexp.replace(&key, "").into_owned();

                        match frequency_map.entry(key) {
                            Entry::Occupied(mut entry) => {
                                entry.get_mut().frequency += 1;
                            }
                            Entry::Vacant(entry) => {
                                let message = match handlebars.render("slack", &new_slack_params_map(e)) {
                                    Ok(msg) => msg,
                                    Err(e) => {
                                        error!("failed to render slack template: {}", e);
                                        continue;
                                    }
                                };
                                entry.insert(Message::new(message, 1));
                            }
                        }
                    }

                    for (_, message) in frequency_map {
                        match serde_json::to_string(&Slack::new(message.text,message.frequency))
                        {
                            Ok(message) => {
                                match self
                                    .client
                                    .post(&self.conf.slack.webhook_url)
                                    .header(CONTENT_TYPE, JSON_TYPE)
                                    .body(message.clone())
                                    .send()
                                    .await {
                                        Ok(resp) => {
                                            let status = resp.status();
                                            match resp.text().await {
                                                Ok(resp_text) => {
                                                    debug!(
                                                        "alert sent: {}, status: {}, resp: {}",
                                                        message, status, resp_text
                                                    );
                                                }
                                                Err(e) => {
                                                    error!("failed to read response body: {}, status: {}", e, status);
                                                }
                                            }
                                        },
                                        Err(err) => {
                                            error!("error sending alert: {}", err);
                                        }
                                }
                            }
                            Err(e) => {
                                error!("error serializing event: {}", e);
                            }
                        }
                    }
                }
                _ = notify.notified() => {
                    log::info!("sender received shutdown signal");

                    break
                }
            }
        }
    }

    pub async fn send(&self, event: Vec<Event>) {
        match self.sender.send(event) {
            Ok(_) => {}
            Err(err) => {
                error!("error sending event to channel: {}", err);
            }
        }
    }
}

fn new_slack_params_map(e: Event) -> HashMap<String, String> {
    HashMap::from([
        ("id".to_string(), e.id),
        ("message".to_string(), e.message),
        ("timestamp".to_string(), e.timestamp),
        ("pod_name".to_string(), e.meta.pod_name),
        ("namespace".to_string(), e.meta.namespace),
        ("container_name".to_string(), e.meta.container_name),
        ("pod_id".to_string(), e.meta.pod_id),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::event::Meta;

    #[test]
    fn test_rfc3339_regex_matches_standard_format() {
        let regexp = Regex::new(RFC3339_REGEX).unwrap();

        assert!(regexp.is_match("2024-01-15T10:30:00Z"));
        assert!(regexp.is_match("2024-12-31T23:59:59Z"));
        assert!(regexp.is_match("2024-01-01T00:00:00Z"));
    }

    #[test]
    fn test_rfc3339_regex_matches_with_nanoseconds() {
        let regexp = Regex::new(RFC3339_REGEX).unwrap();

        assert!(regexp.is_match("2024-01-15T10:30:00.123Z"));
        assert!(regexp.is_match("2024-01-15T10:30:00.123456Z"));
        assert!(regexp.is_match("2024-01-15T10:30:00.123456789Z"));
    }

    #[test]
    fn test_rfc3339_regex_replacement() {
        let regexp = Regex::new(RFC3339_REGEX).unwrap();

        let input = "Error at 2024-01-15T10:30:00Z in production";
        let result = regexp.replace(input, "").into_owned();
        assert_eq!(result, "Error at  in production");

        let input_with_ns = "Log: 2024-01-15T10:30:00.123456789Z - failed";
        let result = regexp.replace(input_with_ns, "").into_owned();
        assert_eq!(result, "Log:  - failed");
    }

    #[test]
    fn test_rfc3339_regex_no_match() {
        let regexp = Regex::new(RFC3339_REGEX).unwrap();

        assert!(!regexp.is_match("2024-01-15"));
        assert!(!regexp.is_match("10:30:00"));
        assert!(!regexp.is_match("not a timestamp"));
        assert!(!regexp.is_match("2024/01/15T10:30:00Z"));
    }

    #[test]
    fn test_new_slack_params_map() {
        let event = Event::new(
            "event-123".to_string(),
            "Error occurred".to_string(),
            "2024-01-15T10:30:00Z".to_string(),
            Meta::new(
                "my-pod".to_string(),
                "production".to_string(),
                "app".to_string(),
                "pod-uuid".to_string(),
            ),
        );

        let params = new_slack_params_map(event);

        assert_eq!(params.get("id").unwrap(), "event-123");
        assert_eq!(params.get("message").unwrap(), "Error occurred");
        assert_eq!(params.get("timestamp").unwrap(), "2024-01-15T10:30:00Z");
        assert_eq!(params.get("pod_name").unwrap(), "my-pod");
        assert_eq!(params.get("namespace").unwrap(), "production");
        assert_eq!(params.get("container_name").unwrap(), "app");
        assert_eq!(params.get("pod_id").unwrap(), "pod-uuid");
    }

    #[test]
    fn test_message_aggregation_key_generation() {
        let regexp = Regex::new(RFC3339_REGEX).unwrap();

        let message1 = "Error at 2024-01-15T10:30:00Z";
        let namespace1 = "prod";
        let key1 = format!("{}-{}", message1, namespace1);
        let normalized_key1 = regexp.replace(&key1, "").into_owned();

        let message2 = "Error at 2024-01-15T11:45:30Z";
        let namespace2 = "prod";
        let key2 = format!("{}-{}", message2, namespace2);
        let normalized_key2 = regexp.replace(&key2, "").into_owned();

        assert_eq!(normalized_key1, normalized_key2);
        assert_eq!(normalized_key1, "Error at -prod");
    }

    #[test]
    fn test_message_aggregation_different_namespaces() {
        let regexp = Regex::new(RFC3339_REGEX).unwrap();

        let key1 = regexp.replace("Error-production", "").into_owned();
        let key2 = regexp.replace("Error-staging", "").into_owned();

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_sender_new_creates_valid_sender() {
        let conf = create_test_config();
        let result = Sender::new(conf);
        assert!(result.is_ok());
    }

    fn create_test_config() -> crate::conf::Conf {
        crate::conf::Conf {
            is_debug: false,
            storage: crate::conf::Storage {
                host: "https://es.example.com".to_string(),
                port: 9200,
                index_name: "logs".to_string(),
                api_prefix: "/".to_string(),
                use_auth: false,
                username: String::new(),
                password: String::new(),
            },
            watch_interval: 60,
            query_string: "level:error".to_string(),
            slack: crate::conf::Slack {
                webhook_url: "https://hooks.slack.com/services/xxx/yyy/zzz".to_string(),
            },
        }
    }
}
