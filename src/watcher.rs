use std::collections::HashMap;
use std::ops::Sub;
use std::sync::Arc;
use std::time::Duration;

use chrono::TimeDelta;
use handlebars::Handlebars;
use log::error;
use reqwest::Client;
use tokio::sync::Notify;
use tokio::time;

use crate::entities::event::{Event, Meta};
use crate::entities::response::Root;
use crate::sender::Sender;
use crate::Conf;

const CONTENT_TYPE: &str = "Content-Type";
const JSON_TYPE: &str = "application/json";

pub struct Watcher {
    conf: Conf,
    sender: Arc<Sender>,
    client: Client,
}

impl Watcher {
    pub fn new(conf: Conf, sender: Arc<Sender>) -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| format!("failed to create HTTP client: {}", e))?;

        Ok(Watcher {
            conf,
            sender,
            client,
        })
    }

    pub async fn run(&mut self, notify: Arc<Notify>) {
        let mut handlebars = Handlebars::new();

        if let Err(e) =
            handlebars.register_template_string("query", include_str!("templates/query.hbs"))
        {
            error!("failed to register query template: {}", e);
            return;
        }

        let mut start_time = chrono::Utc::now();

        let mut ticker = time::interval(Duration::from_secs(self.conf.watch_interval));

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let map = HashMap::from([
                        ("query".to_string(), self.conf.query_string.clone()),
                        (
                            "date".to_string(),
                            start_time.clone().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                        ),
                    ]);

                    let query = match handlebars.render("query", &map) {
                        Ok(q) => q,
                        Err(e) => {
                            error!("failed to render query template: {}", e);
                            continue;
                        }
                    };

                    let url = format!(
                        "{}:{}{}{}/_search",
                        self.conf.storage.host,
                        self.conf.storage.port,
                        self.conf.storage.api_prefix,
                        self.conf.storage.index_name
                    );

                    let mut req = self
                        .client
                        .post(url)
                        .body(query)
                        .header(CONTENT_TYPE, JSON_TYPE);

                    if self.conf.storage.use_auth {
                        req = req.basic_auth(
                            &self.conf.storage.username,
                            Some(&self.conf.storage.password),
                        );
                    }

                    match req.send().await {
                        Ok(resp) => {
                            let resp_text = match resp.text().await {
                                Ok(text) => text,
                                Err(e) => {
                                    error!("failed to read response body: {}", e);
                                    continue;
                                }
                            };

                            match serde_json::from_str::<Root>(&resp_text) {
                                Ok(resp) => {
                                    if resp.hits.hits.is_none() || resp.hits.total.value == 0 {
                                        match TimeDelta::try_seconds(10) {
                                            Some(delta) => {
                                                start_time = chrono::Utc::now().sub(delta);
                                            }
                                            None => {
                                                error!("failed to create 10 second time delta");
                                                start_time = chrono::Utc::now();
                                            }
                                        }
                                        continue;
                                    }

                                    let hits = match resp.hits.hits {
                                        Some(hits) => hits,
                                        None => {
                                            error!("hits is None despite value > 0");
                                            continue;
                                        }
                                    };

                                    let mut events: Vec<Event> = Vec::new();

                                    for hit in hits {
                                        let mut timestamp = String::new();

                                        if let Some(ts) = hit.source.timestamp { // Elasticsearch
                                            timestamp = ts;
                                        } else if let Some(ts) = hit.timestamp { // ZincSearch
                                            timestamp = ts;
                                        }

                                        events.push(Event::new(
                                            hit.id,
                                            hit.source.message,
                                            timestamp,
                                            Meta::new(
                                                hit.source.pod_name,
                                                hit.source.namespace,
                                                hit.source.container_name,
                                                hit.source.pod_id,
                                            ),
                                        ))
                                    }

                                    self.sender.send(events).await;
                                }
                                Err(err) => {
                                    error!("json decode error: {}", err);
                                }
                            }
                        }
                        Err(err) => {
                            error!("query failed with error: {}", err);
                        }
                    }
                }
                _ = notify.notified() => {
                    log::info!("watcher received shutdown signal");

                    break
                }
            }
        }
    }
}
