use std::collections::HashMap;
use std::ops::Sub;
use std::sync::Arc;
use std::time::Duration;

use handlebars::{no_escape, Handlebars};
use log::{debug, error};
use reqwest::Client;
use tokio::sync::{broadcast, Notify, RwLock};
use tokio::time;

use crate::events::Event;
use crate::slack::Slack;
use crate::Conf;

const SEND_QUEUE_SIZE: usize = 1024;
const CLEANUP_INTERVAL: i64 = 3600;
const EVENTS_SIZE_THRESHOLD: usize = 100000;

const CONTENT_TYPE: &str = "Content-Type";
const JSON_TYPE: &str = "application/json";

pub struct Sender {
    conf: Conf,
    sender: broadcast::Sender<Event>,
    client: Client,
    sent: RwLock<HashMap<String, i64>>,
}

impl Sender {
    pub fn new(conf: Conf) -> Self {
        let (tx, _) = broadcast::channel(SEND_QUEUE_SIZE);

        Sender {
            conf,
            sender: tx,
            client: Client::new(),
            sent: RwLock::new(HashMap::new()),
        }
    }

    pub async fn run(&self, notify: Arc<Notify>) {
        let mut handlebars = Handlebars::new();
        handlebars.register_escape_fn(no_escape);

        handlebars
            .register_template_string("slack", include_str!("templates/slack.hbs"))
            .unwrap();

        let mut rx = self.sender.subscribe();

        let mut ticker = time::interval(Duration::from_secs(CLEANUP_INTERVAL as u64));

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let mut map = self.sent.write().await;

                    if map.len() > EVENTS_SIZE_THRESHOLD {
                        let cleanup_treshold =  chrono::Utc::now().sub(chrono::Duration::seconds(CLEANUP_INTERVAL)).timestamp();

                        map.retain(|_, v| *v > cleanup_treshold);

                        debug!("cleanup done");
                    }
                }
                event = rx.recv() => {
                    if event.is_err() {
                        error!("error receiving event: {}", event.err().unwrap());

                        break;
                    }

                    let e = event.unwrap();

                    if self.sent.read().await.contains_key(e.id.clone().as_str()) {
                        continue;
                    }

                    self.sent.write().await.insert(e.id.clone(), chrono::Utc::now().timestamp());

                    let map = HashMap::from([
                        ("id".to_string(), e.id),
                        ("message".to_string(), e.message),
                        ("timestamp".to_string(), e.timestamp),
                        ("pod_name".to_string(), e.meta.pod_name),
                        ("namespace".to_string(), e.meta.namespace),
                        ("container_name".to_string(), e.meta.container_name),
                        ("pod_id".to_string(), e.meta.pod_id),
                    ]);

                    let message = handlebars.render("slack", &map).unwrap();

                    match serde_json::to_string(&Slack{
                        text: message,
                    })
                    {
                        Ok(message) => {
                            match self
                                .client
                                .post(self.conf.slack.webhook_url.clone())
                                .header(CONTENT_TYPE, JSON_TYPE)
                                .body(message.clone())
                                .send()
                                .await {
                                    Ok(resp) => {
                                        debug!(
                                            "alert sent: {}, status: {}, resp: {}",
                                            message, resp.status().to_string(),
                                            resp.text().await.unwrap().as_str()
                                        );
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
                _ = notify.notified() => {
                    log::info!("sender received shutdown signal");

                    break
                }
            }
        }
    }

    pub async fn send(&self, event: Event) {
        match self.sender.send(event) {
            Ok(_) => {}
            Err(err) => {
                error!("error sending event to channel: {}", err);
            }
        }
    }
}
