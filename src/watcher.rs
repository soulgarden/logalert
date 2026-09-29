use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::watch;
use tokio::time::{self, MissedTickBehavior};

use crate::entities::event::{Event, Meta};
use crate::entities::response::Root;
use crate::sender::Sender;
use crate::signals::{is_shutdown, wait_for_shutdown};
use crate::Conf;

const PAGE_SIZE: usize = 50;
const MAX_RESULTS: usize = 10_000;
const OVERLAP: TimeDelta = TimeDelta::seconds(10);

#[cfg(test)]
mod regression_tests;

#[derive(Clone, Copy, Debug, PartialEq)]
struct SearchWindow {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
}

pub struct Watcher {
    conf: Conf,
    client: Client,
    start_time: DateTime<Utc>,
    pending_window: Option<SearchWindow>,
}

impl Watcher {
    pub fn new(conf: Conf) -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| format!("failed to create search client: {e}"))?;
        Ok(Self {
            conf,
            client,
            start_time: Utc::now() - OVERLAP,
            pending_window: None,
        })
    }

    pub async fn run(&mut self, sender: &mut Sender, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = time::interval(Duration::from_secs(self.conf.watch_interval));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                biased;
                _ = wait_for_shutdown(&mut shutdown) => break,
                _ = ticker.tick() => {}
            }
            match self.poll(sender, &mut shutdown).await {
                Ok(true) => {}
                Ok(false) => break,
                Err(error) => log::error!("search window incomplete: {error}"),
            }
        }
    }

    async fn poll(
        &mut self,
        sender: &mut Sender,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<bool, String> {
        let window = *self.pending_window.get_or_insert_with(|| SearchWindow {
            start: self.start_time,
            end: Utc::now(),
        });
        if !self.process_window(window, sender, shutdown).await? {
            return Ok(false);
        }
        self.start_time = window.end - OVERLAP;
        self.pending_window = None;
        Ok(true)
    }

    async fn process_window(
        &self,
        window: SearchWindow,
        sender: &mut Sender,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<bool, String> {
        let mut offset = 0;
        let mut expected_total = None;
        let mut seen = HashSet::new();
        loop {
            if is_shutdown(shutdown) {
                return Ok(false);
            }
            let response = self.fetch_page(window, offset).await?;
            // The request may have completed after a shutdown signal.
            if is_shutdown(shutdown) {
                return Ok(false);
            }
            if response.timed_out || response.shards.as_ref().is_some_and(|s| s.failed > 0) {
                return Err("search returned a partial response".into());
            }
            if response
                .error
                .as_ref()
                .is_some_and(|error| !error.is_null() && error.as_str() != Some(""))
            {
                return Err("search returned an error in the response body".into());
            }
            let total = usize::try_from(response.hits.total.value)
                .map_err(|_| "search returned a negative hit count".to_string())?;
            if total > MAX_RESULTS
                || response
                    .hits
                    .total
                    .relation
                    .as_deref()
                    .is_some_and(|r| r != "eq")
            {
                return Err(format!("search exceeds the exact {MAX_RESULTS}-hit window limit; narrow the query or polling interval"));
            }
            if expected_total.is_some_and(|expected| expected != total) {
                return Err("search results changed during pagination; retrying the window".into());
            }
            expected_total = Some(total);
            let hits = response.hits.hits.unwrap_or_default();
            if hits.len() > PAGE_SIZE
                || offset + hits.len() > total
                || (hits.is_empty() && offset < total)
            {
                return Err("search returned an incomplete or inconsistent page".into());
            }
            let count = hits.len();
            let mut events = Vec::with_capacity(count);
            for hit in hits {
                if !seen.insert((hit.index.clone(), hit.id.clone())) {
                    return Err("search repeated a document during pagination".into());
                }
                let timestamp = hit.source.timestamp.or(hit.timestamp).unwrap_or_default();
                let mut event = Event::new(
                    hit.id,
                    hit.source.message,
                    timestamp,
                    Meta::new(
                        hit.source.pod_name,
                        hit.source.namespace,
                        hit.source.container_name,
                        hit.source.pod_id,
                    ),
                );
                event.index = hit.index;
                events.push(event);
            }
            if !sender.send(events, shutdown).await? {
                return Ok(false);
            }
            offset += count;
            if offset == total {
                return Ok(true);
            }
        }
    }

    async fn fetch_page(&self, window: SearchWindow, offset: usize) -> Result<Root, String> {
        let url = format!(
            "{}:{}{}{}/_search",
            self.conf.storage.host,
            self.conf.storage.port,
            self.conf.storage.api_prefix,
            self.conf.storage.index_name
        );
        // Keep replica selection stable for equal timestamps. Pagination
        // guards still reject inconsistent results after cluster changes.
        let mut url =
            reqwest::Url::parse(&url).map_err(|error| format!("invalid search URL: {error}"))?;
        url.query_pairs_mut().append_pair("preference", "logalert");
        let mut request =
            self.client
                .post(url)
                .json(&build_query(&self.conf.query_string, window, offset));
        if self.conf.storage.use_auth {
            request = request.basic_auth(
                &self.conf.storage.username,
                Some(&self.conf.storage.password),
            );
        }
        request
            .send()
            .await
            .and_then(|response| response.error_for_status())
            .map_err(|e| format!("search request failed: {}", e.without_url()))?
            .json::<Root>()
            .await
            .map_err(|e| format!("invalid search response: {}", e.without_url()))
    }
}

fn build_query(query: &str, window: SearchWindow, offset: usize) -> Value {
    json!({
        "query": {"bool": {"filter": [
            {"range": {"@timestamp": {"gte": window.start.to_rfc3339(), "lt": window.end.to_rfc3339()}}},
            {"query_string": {"query": query}}
        ]}},
        "from": offset,
        "size": PAGE_SIZE,
        "track_total_hits": true,
        "sort": [{"@timestamp": {"order": "asc"}}]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_construction() {
        let window = SearchWindow {
            start: "2024-01-15T10:30:00Z".parse().unwrap(),
            end: "2024-01-15T10:31:00Z".parse().unwrap(),
        };
        let query = build_query("level:error AND namespace:production", window, 50);
        assert_eq!(query["from"], 50);
        assert_eq!(query["size"], PAGE_SIZE);
        assert_eq!(
            query["query"]["bool"]["filter"][1]["query_string"]["query"],
            "level:error AND namespace:production"
        );
        assert_eq!(
            query["query"]["bool"]["filter"][0]["range"]["@timestamp"]["gte"],
            window.start.to_rfc3339()
        );
        assert_eq!(
            query["query"]["bool"]["filter"][0]["range"]["@timestamp"]["lt"],
            window.end.to_rfc3339()
        );
    }

    #[test]
    fn test_url_construction() {
        let host = "https://elasticsearch.example.com";
        let port = 9200u16;
        let api_prefix = "/";
        let index_name = "logs";

        let url = format!("{}:{}{}{}/_search", host, port, api_prefix, index_name);

        assert_eq!(url, "https://elasticsearch.example.com:9200/logs/_search");
    }

    #[test]
    fn test_url_construction_with_api_prefix() {
        let host = "https://elasticsearch.example.com";
        let port = 9200u16;
        let api_prefix = "/api/v1/";
        let index_name = "logs";

        let url = format!("{}:{}{}{}/_search", host, port, api_prefix, index_name);

        assert_eq!(
            url,
            "https://elasticsearch.example.com:9200/api/v1/logs/_search"
        );
    }

    #[test]
    fn test_event_creation_from_hit() {
        let id = "hit-123".to_string();
        let message = "Error occurred".to_string();
        let timestamp = "2024-01-15T10:30:00Z".to_string();
        let meta = Meta::new(
            "pod-1".to_string(),
            "production".to_string(),
            "app".to_string(),
            "uuid-1".to_string(),
        );

        let event = Event::new(id.clone(), message.clone(), timestamp.clone(), meta);

        assert_eq!(event.id, id);
        assert_eq!(event.message, message);
        assert_eq!(event.timestamp, timestamp);
        assert_eq!(event.meta.pod_name, "pod-1");
        assert_eq!(event.meta.namespace, "production");
    }

    #[test]
    fn test_rfc3339_date_format() {
        let now = chrono::Utc::now();
        let formatted = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        assert!(formatted.ends_with('Z'));
        assert!(formatted.contains('T'));
        assert_eq!(formatted.len(), 20);
    }

    #[test]
    fn test_time_delta_10_seconds() {
        let delta = TimeDelta::try_seconds(10);
        assert!(delta.is_some());

        let now = chrono::Utc::now();
        let past = now - delta.unwrap();

        assert!(past < now);
    }
}
