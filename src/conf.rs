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

#[derive(Debug, Deserialize, Clone)]
pub struct Conf {
    pub is_debug: bool,
    pub storage: Storage,
    pub watch_interval: u64,
    pub query_string: String,
    pub slack: Slack,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Storage {
    pub host: String,
    pub port: u16,
    pub index_name: String,
    pub api_prefix: String,
    pub use_auth: bool,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize, Clone)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_valid_config() -> Conf {
        Conf {
            is_debug: false,
            storage: Storage {
                host: "https://elasticsearch.example.com".to_string(),
                port: 9200,
                index_name: "logs".to_string(),
                api_prefix: "/".to_string(),
                use_auth: false,
                username: String::new(),
                password: String::new(),
            },
            watch_interval: 60,
            query_string: "level:error".to_string(),
            slack: Slack {
                webhook_url: "https://hooks.slack.com/services/xxx/yyy/zzz".to_string(),
            },
        }
    }

    fn write_config_file(config: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(config.as_bytes()).unwrap();
        file
    }

    #[test]
    fn test_valid_config() {
        let config = create_valid_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_valid_config_from_file() {
        let config_json = r#"{
            "is_debug": false,
            "storage": {
                "host": "https://es.example.com",
                "port": 9200,
                "index_name": "logs",
                "api_prefix": "/",
                "use_auth": false,
                "username": "",
                "password": ""
            },
            "watch_interval": 60,
            "query_string": "level:error",
            "slack": {
                "webhook_url": "https://hooks.slack.com/services/T00/B00/xxx"
            }
        }"#;

        let file = write_config_file(config_json);
        env::set_var("CFG_PATH", file.path());

        let result = Conf::new();
        assert!(result.is_ok());

        env::remove_var("CFG_PATH");
    }

    #[test]
    fn test_invalid_watch_interval_zero() {
        let mut config = create_valid_config();
        config.watch_interval = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .contains("watch_interval must be greater than 0"));
    }

    #[test]
    fn test_invalid_watch_interval_too_large() {
        let mut config = create_valid_config();
        config.watch_interval = 3601;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .contains("should not exceed 3600 seconds"));
    }

    #[test]
    fn test_valid_watch_interval_boundary() {
        let mut config = create_valid_config();
        config.watch_interval = 1;
        assert!(config.validate().is_ok());

        config.watch_interval = 3600;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_empty_query_string() {
        let mut config = create_valid_config();
        config.query_string = "   ".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .contains("query_string cannot be empty"));
    }

    #[test]
    fn test_invalid_storage_host_empty() {
        let mut config = create_valid_config();
        config.storage.host = "".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .contains("storage host cannot be empty"));
    }

    #[test]
    fn test_invalid_storage_host_no_protocol() {
        let mut config = create_valid_config();
        config.storage.host = "elasticsearch.example.com".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .contains("must start with http:// or https://"));
    }

    #[test]
    fn test_invalid_storage_port_zero() {
        let mut config = create_valid_config();
        config.storage.port = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .contains("storage port must be greater than 0"));
    }

    #[test]
    fn test_invalid_storage_index_name_empty() {
        let mut config = create_valid_config();
        config.storage.index_name = "".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .contains("storage index_name cannot be empty"));
    }

    #[test]
    fn test_invalid_storage_api_prefix_empty() {
        let mut config = create_valid_config();
        config.storage.api_prefix = "".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .contains("storage api_prefix cannot be empty"));
    }

    #[test]
    fn test_auth_enabled_missing_username() {
        let mut config = create_valid_config();
        config.storage.use_auth = true;
        config.storage.username = "".to_string();
        config.storage.password = "secret".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .contains("username cannot be empty when use_auth is true"));
    }

    #[test]
    fn test_auth_enabled_missing_password() {
        let mut config = create_valid_config();
        config.storage.use_auth = true;
        config.storage.username = "admin".to_string();
        config.storage.password = "".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .contains("password cannot be empty when use_auth is true"));
    }

    #[test]
    fn test_auth_enabled_valid_credentials() {
        let mut config = create_valid_config();
        config.storage.use_auth = true;
        config.storage.username = "admin".to_string();
        config.storage.password = "secret".to_string();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_slack_webhook_empty() {
        let mut config = create_valid_config();
        config.slack.webhook_url = "".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .contains("slack webhook_url cannot be empty"));
    }

    #[test]
    fn test_invalid_slack_webhook_not_url() {
        let mut config = create_valid_config();
        config.slack.webhook_url = "not-a-url".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("must be a valid URL"));
    }

    #[test]
    fn test_invalid_slack_webhook_wrong_domain() {
        let mut config = create_valid_config();
        config.slack.webhook_url = "https://example.com/webhook".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .contains("must be a valid Slack webhook URL"));
    }

    #[test]
    fn test_missing_config_file() {
        env::set_var("CFG_PATH", "/nonexistent/path/config.json");
        let result = Conf::new();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .contains("can't open config.json file"));
        env::remove_var("CFG_PATH");
    }

    #[test]
    fn test_invalid_json_config() {
        let file = write_config_file("{ invalid json }");
        env::set_var("CFG_PATH", file.path());

        let result = Conf::new();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .contains("can't parse config.json file"));

        env::remove_var("CFG_PATH");
    }
}
