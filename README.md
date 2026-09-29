# logalert

![Tests and linters](https://github.com/soulgarden/logalert/actions/workflows/main.yml/badge.svg)

A lightweight, memory-efficient Rust application that monitors Elasticsearch/ZincSearch for specific log events and delivers real-time alerts to Slack. Designed for high-performance log monitoring in production environments with minimal resource overhead.

## Features

- **Low Resource Usage**: Optimized for minimal CPU and memory consumption
- **Real-time Monitoring**: Continuous polling with configurable intervals
- **Event Deduplication**: Intelligent message aggregation to prevent spam
- **JSON Queries**: Search strings are serialized without changing their contents
- **Robust Error Handling**: Comprehensive validation and graceful failure recovery
- **Cloud Native**: Ready for Kubernetes deployment with Helm charts

**Compatibility**: Elasticsearch 7.x, Kubernetes 1.14+

## Architecture

Logalert runs one asynchronous processing loop:

```
Read a search page → aggregate events → deliver to Slack → read the next page
```

- `Watcher` queries fixed time windows in ascending timestamp order, 50 records at a time. It advances the window only after every page has been delivered. Windows overlap by 10 seconds to include recently indexed events.
- `Sender` groups messages by normalized text and namespace. It records document IDs only after Slack returns a successful acknowledgement. IDs include the index name and remain in memory for one hour.
- Network failures, HTTP 429, and server errors are retried up to three times per delivery call. `Retry-After` is honored, including across retries of the search window. Other HTTP errors return immediately.
- A failed window is retried on the next polling tick. Successful groups are deduplicated during replay. Reading waits for delivery, so there is no internal queue to overflow.
- SIGINT and SIGTERM stop new work and allow the current request to finish. Shutdown has a 15-second deadline; expiry returns a nonzero exit status.

Pagination uses `from`/`size` for Elasticsearch and ZincSearch compatibility. A fixed `preference` keeps Elasticsearch replica selection consistent while cluster state is unchanged. A window is limited to 10,000 hits. Partial responses, changing totals, repeated document IDs, and inconsistent pages fail the window instead of advancing it. The same window is retried until it succeeds; exceeding the limit requires operator intervention to narrow the query. This is not snapshot pagination: concurrent indexing or deletion can still affect the results. Events arriving more than 10 seconds late may fall outside the overlap.

Delivery state is in memory. Restarting does not resume an unfinished window, and an ambiguous network failure can produce a duplicate Slack notification. Durable delivery across restarts is outside this service's current contract.

## Configuration

Create a `config.json` file or set the `CFG_PATH` environment variable:

```json
{
  "is_debug": true,
  "storage": {
    "host": "http://elasticsearch.example.com",
    "port": 9200,
    "index_name": "logs-*",
    "api_prefix": "/",
    "use_auth": true,
    "username": "admin", 
    "password": "password"
  },
  "watch_interval": 60,
  "query_string": "level:error OR status:5*",
  "slack": {
    "webhook_url": "https://hooks.slack.com/services/YOUR/SLACK/WEBHOOK"
  }
}
```

### Configuration Parameters

- **`watch_interval`**: Polling interval in seconds (1-3600)
- **`query_string`**: Elasticsearch query string syntax for matching events
- **`storage.index_name`**: Elasticsearch index pattern to search
- **`storage.api_prefix`**: API endpoint prefix (usually `/` for ES, `/api` for ZincSearch)
- **`slack.webhook_url`**: Slack incoming webhook URL for notifications

## Installation

### Kubernetes with Helm

```bash
# Create namespace
make create_namespace

# Install application  
make helm_install

# Upgrade existing installation
make helm_upgrade
```

### Docker

```bash
# Build image locally
make build

# Build and push image
make push

# Run container
docker run --rm -v "$(pwd)/config.json:/config.json:ro" "soulgarden/logalert:$(cat VERSION)"
```

### From Source

```bash
# Build release binary
cargo build --release

# Run with config
CFG_PATH=./config.json ./target/release/logalert
```

## Development

```bash
# Format code
make fmt

# Run linting
make lint

# Run linting with auto-fix
make lint_fix

# Run tests
make test
```

## Performance Characteristics

- **Memory Usage**: ~5-15MB typical runtime footprint
- **CPU Usage**: Minimal baseline, scales with event volume
- **Network**: Efficient HTTP/1.1 with connection pooling  
- **Storage**: No persistent storage required - purely in-memory operation
