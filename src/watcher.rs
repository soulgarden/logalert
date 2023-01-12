use std::collections::HashMap;
use std::ops::Sub;
use std::sync::Arc;
use std::time::Duration;

use handlebars::Handlebars;
use log::error;
use reqwest::Client;
use tokio::sync::Notify;
use tokio::time;

use crate::events::{Event, Meta};
use crate::response::Root;
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
    pub fn new(conf: Conf, sender: Arc<Sender>) -> Self {
        Watcher {
            conf,
            sender,
            client: Client::new(),
        }
    }

    pub async fn run(&mut self, notify: Arc<Notify>) {
        let mut handlebars = Handlebars::new();

        handlebars
            .register_template_string("query", include_str!("templates/query.hbs"))
            .unwrap();

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

                    let query = handlebars.render("query", &map).unwrap();

                    let resp = self
                        .client
                        .post(
                            self.conf.storage.host.clone()
                                + ":"
                                + self.conf.storage.port.to_string().as_str()
                                + self.conf.storage.api_prefix.to_string().as_str()
                                + self.conf.storage.index_name.as_str()
                                + "/_search",
                        )
                        .body(query.clone())
                        .header(CONTENT_TYPE, JSON_TYPE)
                        .send()
                        .await;

                    match resp {
                        Ok(resp) => {
                            let resp_text = resp.text().await.unwrap();

                            let resp = serde_json::from_str::<Root>(resp_text.as_str());

                            match resp {
                                Ok(resp) => {
                                    let hits = resp.hits.hits;

                                    if resp.hits.total.value == 0 {
                                        start_time = chrono::Utc::now().sub(chrono::Duration::seconds(10));
                                    }

                                    for hit in hits {
                                        self.sender
                                            .send(Event::new(
                                                hit.id,
                                                hit.source.message,
                                                hit.source.timestamp,
                                                Meta::new(
                                                    hit.source.pod_name,
                                                    hit.source.namespace,
                                                    hit.source.container_name,
                                                    hit.source.pod_id,
                                                ),
                                            ))
                                            .await;
                                    }
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
