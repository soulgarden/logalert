//! Local HTTP fixtures shared by the delivery and polling regression tests.

use std::{future::Future, time::Duration};

use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::timeout,
};

use crate::{conf, entities::event::Event, entities::event::Meta, Conf};

pub const TEST_TIMEOUT: Duration = Duration::from_secs(3);

/// Aborts background work even when an assertion panics.
pub struct TestTask(JoinHandle<()>);

impl TestTask {
    pub fn spawn(future: impl Future<Output = ()> + Send + 'static) -> Self {
        Self(tokio::spawn(future))
    }

    pub async fn finish(&mut self, expectation: &str) {
        timeout(TEST_TIMEOUT, &mut self.0)
            .await
            .unwrap_or_else(|_| panic!("{expectation}"))
            .expect("background task panicked");
    }
}

impl Drop for TestTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub struct Request {
    pub target: String,
    body: String,
    response: oneshot::Sender<(u16, String, String)>,
}

impl Request {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).expect("outgoing request must contain valid JSON")
    }

    pub fn respond(self, status: u16, body: impl Into<String>) {
        self.respond_with_headers(status, body, &[]);
    }

    pub fn respond_with_headers(
        self,
        status: u16,
        body: impl Into<String>,
        headers: &[(&str, &str)],
    ) {
        let headers = headers
            .iter()
            .map(|(key, value)| format!("{key}: {value}\r\n"))
            .collect::<String>();
        self.response
            .send((status, body.into(), headers))
            .expect("HTTP connection closed before the test responded");
    }
}

pub struct MockHttp {
    pub port: u16,
    requests: mpsc::UnboundedReceiver<Request>,
    _task: TestTask,
}

impl MockHttp {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, requests) = mpsc::unbounded_channel();
        let task = TestTask::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let (target, body) = read_request(&mut stream).await;
                let (response, rx) = oneshot::channel();
                if tx
                    .send(Request {
                        target,
                        body,
                        response,
                    })
                    .is_err()
                {
                    break;
                }
                // Holding Request keeps the HTTP call in flight. Dropping it
                // closes the socket without a response to simulate a failure.
                if let Ok((status, body, headers)) = rx.await {
                    let response = format!(
                        "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                }
            }
        });
        Self {
            port,
            requests,
            _task: task,
        }
    }

    pub async fn next(&mut self, expectation: &str) -> Request {
        timeout(TEST_TIMEOUT, self.requests.recv())
            .await
            .unwrap_or_else(|_| panic!("{expectation}"))
            .expect("mock HTTP server stopped")
    }
}

// reqwest sends these String bodies with Content-Length. The fixture only
// implements that framing and closes each connection after one response.
async fn read_request(stream: &mut TcpStream) -> (String, String) {
    let mut bytes = Vec::new();
    let (offset, length) = loop {
        let mut buffer = [0; 4096];
        let n = stream.read(&mut buffer).await.unwrap();
        assert!(n > 0, "client closed an incomplete HTTP request");
        bytes.extend_from_slice(&buffer[..n]);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .expect("expected Content-Length")
                .trim()
                .parse()
                .unwrap();
            break (end + 4, length);
        }
    };
    while bytes.len() < offset + length {
        let mut buffer = [0; 4096];
        let n = stream.read(&mut buffer).await.unwrap();
        assert!(n > 0, "client closed an incomplete HTTP body");
        bytes.extend_from_slice(&buffer[..n]);
    }
    let headers = String::from_utf8_lossy(&bytes[..offset]);
    let target = headers
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_string();
    (
        target,
        String::from_utf8(bytes[offset..offset + length].to_vec()).unwrap(),
    )
}

pub fn config(port: u16) -> Conf {
    Conf {
        is_debug: false,
        storage: conf::Storage {
            host: "http://127.0.0.1".into(),
            port,
            index_name: "logs".into(),
            api_prefix: "/".into(),
            use_auth: false,
            username: String::new(),
            password: String::new(),
        },
        watch_interval: 1,
        query_string: "level:error".into(),
        slack: conf::Slack {
            webhook_url: format!("http://127.0.0.1:{port}/webhook"),
        },
    }
}

pub fn event(id: &str, message: &str) -> Event {
    Event::new(
        id.into(),
        message.into(),
        "2026-09-28T00:00:00Z".into(),
        Meta::default(),
    )
}
