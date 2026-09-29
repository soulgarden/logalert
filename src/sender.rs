use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use handlebars::{no_escape, Handlebars};
use regex::Regex;
use reqwest::{Client, StatusCode};
use tokio::sync::watch;

use crate::entities::event::Event;
use crate::entities::message::Message;
use crate::entities::slack::Slack;
use crate::signals::{is_shutdown, wait_for_shutdown};
use crate::Conf;

const CACHE_TTL: Duration = Duration::from_secs(3600);
const MAX_ATTEMPTS: u32 = 3;
const RFC3339_REGEX: &str = r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,9})?Z";
type EventKey = (String, String);

#[cfg(test)]
mod regression_tests;

struct Group {
    ids: Vec<EventKey>,
    message: Message,
}

pub struct Sender {
    webhook_url: String,
    client: Client,
    sent: HashMap<EventKey, Instant>,
    regexp: Regex,
    templates: Handlebars<'static>,
    retry_delay: Duration,
    retry_not_before: Option<tokio::time::Instant>,
}

impl Sender {
    pub fn new(conf: Conf) -> Result<Self, String> {
        let mut templates = Handlebars::new();
        templates.register_escape_fn(no_escape);
        templates
            .register_template_string("slack", include_str!("templates/slack.hbs"))
            .map_err(|e| format!("invalid Slack template: {e}"))?;
        Ok(Self {
            webhook_url: conf.slack.webhook_url,
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(10))
                .build()
                .map_err(|e| format!("failed to create Slack client: {e}"))?,
            sent: HashMap::new(),
            regexp: Regex::new(RFC3339_REGEX).map_err(|e| e.to_string())?,
            templates,
            retry_delay: Duration::from_secs(1),
            retry_not_before: None,
        })
    }

    /// Returns false when shutdown interrupts delivery. Only acknowledged IDs
    /// enter the cache, so callers can safely replay an unfinished search window.
    pub async fn send(
        &mut self,
        events: Vec<Event>,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<bool, String> {
        self.sent.retain(|_, time| time.elapsed() < CACHE_TTL);
        let groups = self.aggregate(events)?;
        for group in groups {
            if is_shutdown(shutdown) {
                return Ok(false);
            }
            let payload = Slack::new(group.message.text, group.message.frequency);
            if !self.deliver(&payload, shutdown).await? {
                return Ok(false);
            }
            let now = Instant::now();
            self.sent.extend(group.ids.into_iter().map(|id| (id, now)));
        }
        Ok(true)
    }

    fn aggregate(&self, events: Vec<Event>) -> Result<Vec<Group>, String> {
        let mut seen = HashSet::new();
        let mut positions = HashMap::new();
        let mut groups: Vec<Group> = Vec::new();
        for event in events {
            let id = (event.index.clone(), event.id.clone());
            if self.sent.contains_key(&id) || !seen.insert(id.clone()) {
                continue;
            }
            let key = (
                self.regexp.replace_all(&event.message, "").into_owned(),
                event.meta.namespace.clone(),
            );
            if let Some(&position) = positions.get(&key) {
                let group: &mut Group = &mut groups[position];
                group.ids.push(id);
                group.message.frequency += 1;
            } else {
                let text = self
                    .templates
                    .render("slack", &new_slack_params_map(event))
                    .map_err(|e| format!("failed to render Slack message: {e}"))?;
                positions.insert(key, groups.len());
                groups.push(Group {
                    ids: vec![id],
                    message: Message::new(text, 1),
                });
            }
        }
        Ok(groups)
    }

    async fn deliver(
        &mut self,
        payload: &Slack,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<bool, String> {
        for attempt in 0..MAX_ATTEMPTS {
            if is_shutdown(shutdown) {
                return Ok(false);
            }
            // Keep server cooldowns across calls, including after the final
            // failed attempt when the watcher retries the whole window.
            if let Some(deadline) = self.retry_not_before {
                tokio::select! {
                    biased;
                    _ = wait_for_shutdown(shutdown) => return Ok(false),
                    _ = tokio::time::sleep_until(deadline) => {}
                }
                self.retry_not_before = None;
            }
            let mut delay = self.retry_delay * (1 << attempt);
            let error = match self
                .client
                .post(&self.webhook_url)
                .json(payload)
                .send()
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    if status == StatusCode::OK {
                        match response.text().await {
                            Ok(body) if body.trim() == "ok" => return Ok(true),
                            Ok(_) => {
                                return Err("Slack returned an unexpected acknowledgement".into())
                            }
                            Err(e) => {
                                format!("failed to read Slack acknowledgement: {}", e.without_url())
                            }
                        }
                    } else if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                        if let Some(seconds) = response
                            .headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse::<u64>().ok())
                        {
                            delay = Duration::from_secs(seconds);
                        }
                        format!("Slack returned {status}")
                    } else {
                        return Err(format!("Slack rejected the notification: {status}"));
                    }
                }
                Err(e) => format!("Slack request failed: {}", e.without_url()),
            };
            let deadline = tokio::time::Instant::now()
                .checked_add(delay)
                .ok_or_else(|| "invalid Slack Retry-After interval".to_string())?;
            self.retry_not_before = Some(deadline);
            if attempt + 1 == MAX_ATTEMPTS {
                return Err(format!("{error}; delivery attempts exhausted"));
            }
            log::warn!("{error}; retrying in {} seconds", delay.as_secs_f64());
        }
        unreachable!("MAX_ATTEMPTS is nonzero")
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
