# logalert

![Tests and linters](https://github.com/soulgarden/logalert/actions/workflows/main.yml/badge.svg)

A lightweight, memory-efficient Rust application that monitors Elasticsearch/ZincSearch for specific log events and delivers real-time alerts to Slack. Designed for high-performance log monitoring in production environments with minimal resource overhead.

## Features

- **Low Resource Usage**: Optimized for minimal CPU and memory consumption
- **Real-time Monitoring**: Continuous polling with configurable intervals
- **Event Deduplication**: Intelligent message aggregation to prevent spam
- **Template-based Queries**: Flexible Handlebars templates for search queries
- **Robust Error Handling**: Comprehensive validation and graceful failure recovery
- **Cloud Native**: Ready for Kubernetes deployment with Helm charts

**Compatibility**: Elasticsearch 7.x, Kubernetes 1.14+

## Architecture

Logalert uses an **actor-based architecture** with two main components that run concurrently:

### Core Components

#### 1. **Watcher** (`src/watcher.rs`)
- **Purpose**: Polls Elasticsearch/ZincSearch for new events matching specified criteria
- **Polling Strategy**: Time-based querying using configurable intervals (1-3600 seconds)
- **Query Engine**: Uses Handlebars templates for flexible query construction
- **Data Pipeline**: Processes search results and forwards events to the Sender

**Query Template** (`src/templates/query.hbs`):
```json
{
  "query": {
    "bool": {
      "filter": [
        {
          "range": {
            "@timestamp": { "gte": "{{ date }}" }
          }
        },
        {
          "query_string": {
            "query": "{{ query }}"
          }
        }
      ]
    }
  },
  "size": 50,
  "sort": [{ "@timestamp": { "order": "desc" } }]
}
```

#### 2. **Sender** (`src/sender.rs`)
- **Purpose**: Processes events and delivers Slack notifications
- **Deduplication**: In-memory HashMap tracks sent messages to prevent duplicates
- **Rate Limiting**: Aggregates events by message+namespace to reduce notification spam
- **Cleanup**: Automatic memory cleanup of old event tracking data
- **Template Engine**: Handlebars templates for Slack message formatting

### Data Flow

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Elasticsearch │◄───│     Watcher     │    │     Sender      │
│   /ZincSearch   │    │                 │    │                 │
└─────────────────┘    │ • Polls every N │    │ • Deduplicates  │
                       │   seconds       │───►│ • Aggregates    │
┌─────────────────┐    │ • Queries with  │    │ • Sends to Slack│
│   Config File   │───►│   templates     │    │                 │
└─────────────────┘    │ • Parses events │    └─────────────────┘
                       └─────────────────┘             │
                                                       ▼
                                              ┌─────────────────┐
                                              │      Slack      │
                                              └─────────────────┘
```

### Working Principles

1. **Event-Driven Processing**: Uses Tokio's async runtime for concurrent operation
2. **Memory Management**: In-memory deduplication cache with automatic cleanup
3. **Graceful Shutdown**: Signal handling for clean application termination
4. **Configuration Validation**: Comprehensive input validation prevents runtime errors
5. **HTTP Resilience**: Configurable timeouts and connection limits for reliability

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
# Build and push image
make build

# Run container
docker run -v $(pwd)/config.json:/app/config.json soulgarden/logalert:0.0.10
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
