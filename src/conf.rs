use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::{env, fmt};

use serde::Deserialize;

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

        Ok(conf)
    }
}
