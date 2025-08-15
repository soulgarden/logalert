use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::{env, fmt};

use serde::Deserialize;
use url::Url;

#[derive(Debug, Clone)]
pub struct ConfError {
    pub message: String,
}

impl fmt::Display for ConfError {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::fmt::Result {
        write!(f, "ConfError: {}", self.message)
    }
}

#[derive(Deserialize, Clone)]
pub struct Conf {
    pub is_debug: bool,
    pub storage: Storage,
    pub watch_interval: u64,
    pub query_string: String,
    pub slack: Slack,
}

#[derive(Deserialize, Clone)]
pub struct Storage {
    pub host: String,
    pub port: u16,
    pub index_name: String,
    pub api_prefix: String,
    pub use_auth: bool,
    pub username: String,
    pub password: String,
}

#[derive(Deserialize, Clone)]
pub struct Slack {
    pub webhook_url: String,
}

impl Conf {
    pub fn new() -> Result<Conf, ConfError> {
        let path = match env::var("CFG_PATH") {
            Ok(path) => path,
            Err(_) => "./config.json".to_string(),
        };

        let file = File::open(path).map_err(|e| ConfError {
            message: format!("can't open config.json file, {e}"),
        })?;

        let mut buf_reader = BufReader::new(file);

        let mut contents = String::new();

        buf_reader
            .read_to_string(&mut contents)
            .map_err(|e| ConfError {
                message: format!("can't read config.json file, {e}"),
            })?;

        let conf: Conf = serde_json::from_str(contents.as_str()).map_err(|e| ConfError {
            message: format!("can't parse config.json file, {e}"),
        })?;

        conf.validate()?;

        Ok(conf)
    }

    fn validate(&self) -> Result<(), ConfError> {
        // Validate polling interval
        if self.watch_interval == 0 {
            return Err(ConfError {
                message: "watch_interval must be greater than 0".to_string(),
            });
        }

        if self.watch_interval > 3600 {
            return Err(ConfError {
                message: "watch_interval should not exceed 3600 seconds (1 hour)".to_string(),
            });
        }

        // Validate query string
        if self.query_string.trim().is_empty() {
            return Err(ConfError {
                message: "query_string cannot be empty".to_string(),
            });
        }

        // Validate storage configuration
        self.storage.validate()?;

        // Validate Slack configuration
        self.slack.validate()?;

        Ok(())
    }
}

impl Storage {
    fn validate(&self) -> Result<(), ConfError> {
        // Validate host
        if self.host.trim().is_empty() {
            return Err(ConfError {
                message: "storage host cannot be empty".to_string(),
            });
        }

        // Check that host starts with http:// or https://
        if !self.host.starts_with("http://") && !self.host.starts_with("https://") {
            return Err(ConfError {
                message: "storage host must start with http:// or https://".to_string(),
            });
        }

        // Validate port
        if self.port == 0 {
            return Err(ConfError {
                message: "storage port must be greater than 0".to_string(),
            });
        }

        // Validate index name
        if self.index_name.trim().is_empty() {
            return Err(ConfError {
                message: "storage index_name cannot be empty".to_string(),
            });
        }

        // Validate API prefix
        if self.api_prefix.trim().is_empty() {
            return Err(ConfError {
                message: "storage api_prefix cannot be empty".to_string(),
            });
        }

        // Validate credentials when authentication is enabled
        if self.use_auth {
            if self.username.trim().is_empty() {
                return Err(ConfError {
                    message: "storage username cannot be empty when use_auth is true".to_string(),
                });
            }
            if self.password.trim().is_empty() {
                return Err(ConfError {
                    message: "storage password cannot be empty when use_auth is true".to_string(),
                });
            }
        }

        Ok(())
    }
}

impl Slack {
    fn validate(&self) -> Result<(), ConfError> {
        // Validate webhook URL
        if self.webhook_url.trim().is_empty() {
            return Err(ConfError {
                message: "slack webhook_url cannot be empty".to_string(),
            });
        }

        // Check that it's a valid URL
        match Url::parse(&self.webhook_url) {
            Ok(url) => {
                // Check that it's a Slack webhook URL
                if !url.host_str().unwrap_or("").ends_with("slack.com") {
                    return Err(ConfError {
                        message: "webhook_url must be a valid Slack webhook URL".to_string(),
                    });
                }
            }
            Err(_) => {
                return Err(ConfError {
                    message: "webhook_url must be a valid URL".to_string(),
                });
            }
        }

        Ok(())
    }
}
