use serde_derive::Deserialize;

#[derive(Deserialize)]
pub struct Source {
    pub message: String,
    #[serde(rename(deserialize = "@timestamp"))]
    pub timestamp: Option<String>,
    pub pod_name: String,
    pub namespace: String,
    pub container_name: String,
    pub pod_id: String,
}

#[derive(Deserialize)]
pub struct Struct {
    pub _type: String,
    #[serde(rename(deserialize = "_id"))]
    pub id: String,
    #[serde(rename(deserialize = "@timestamp"))]
    pub timestamp: Option<String>,
    #[serde(rename(deserialize = "_source"))]
    pub source: Source,
}

#[derive(Deserialize)]
pub struct Total {
    pub value: i64,
}

#[derive(Deserialize)]
pub struct Hits {
    pub total: Total,
    pub hits: Option<Vec<Struct>>,
}

#[derive(Deserialize)]
pub struct Root {
    pub hits: Hits,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_es_response_with_hits() {
        let json = r#"{
            "hits": {
                "total": {"value": 2},
                "hits": [
                    {
                        "_type": "_doc",
                        "_id": "abc123",
                        "@timestamp": "2024-01-15T10:30:00Z",
                        "_source": {
                            "message": "Error occurred",
                            "@timestamp": "2024-01-15T10:30:00Z",
                            "pod_name": "app-pod-1",
                            "namespace": "production",
                            "container_name": "app",
                            "pod_id": "pod-uuid-1"
                        }
                    },
                    {
                        "_type": "_doc",
                        "_id": "def456",
                        "_source": {
                            "message": "Another error",
                            "pod_name": "app-pod-2",
                            "namespace": "staging",
                            "container_name": "worker",
                            "pod_id": "pod-uuid-2"
                        }
                    }
                ]
            }
        }"#;

        let root: Root = serde_json::from_str(json).unwrap();

        assert_eq!(root.hits.total.value, 2);
        assert!(root.hits.hits.is_some());

        let hits = root.hits.hits.unwrap();
        assert_eq!(hits.len(), 2);

        assert_eq!(hits[0].id, "abc123");
        assert_eq!(hits[0].source.message, "Error occurred");
        assert_eq!(hits[0].source.pod_name, "app-pod-1");
        assert_eq!(hits[0].source.namespace, "production");
        assert!(hits[0].timestamp.is_some());
        assert!(hits[0].source.timestamp.is_some());

        assert_eq!(hits[1].id, "def456");
        assert!(hits[1].timestamp.is_none());
        assert!(hits[1].source.timestamp.is_none());
    }

    #[test]
    fn test_deserialize_empty_hits() {
        let json = r#"{
            "hits": {
                "total": {"value": 0},
                "hits": []
            }
        }"#;

        let root: Root = serde_json::from_str(json).unwrap();

        assert_eq!(root.hits.total.value, 0);
        assert!(root.hits.hits.is_some());
        assert!(root.hits.hits.unwrap().is_empty());
    }

    #[test]
    fn test_deserialize_null_hits() {
        let json = r#"{
            "hits": {
                "total": {"value": 0}
            }
        }"#;

        let root: Root = serde_json::from_str(json).unwrap();

        assert_eq!(root.hits.total.value, 0);
        assert!(root.hits.hits.is_none());
    }

    #[test]
    fn test_deserialize_zincsearch_timestamp_format() {
        let json = r#"{
            "hits": {
                "total": {"value": 1},
                "hits": [
                    {
                        "_type": "_doc",
                        "_id": "zinc123",
                        "@timestamp": "2024-01-15T10:30:00.123456789Z",
                        "_source": {
                            "message": "ZincSearch log",
                            "pod_name": "zinc-pod",
                            "namespace": "default",
                            "container_name": "zinc",
                            "pod_id": "zinc-uuid"
                        }
                    }
                ]
            }
        }"#;

        let root: Root = serde_json::from_str(json).unwrap();
        let hits = root.hits.hits.unwrap();

        assert_eq!(
            hits[0].timestamp.as_ref().unwrap(),
            "2024-01-15T10:30:00.123456789Z"
        );
    }

    #[test]
    fn test_deserialize_special_characters_in_message() {
        let json = r#"{
            "hits": {
                "total": {"value": 1},
                "hits": [
                    {
                        "_type": "_doc",
                        "_id": "special123",
                        "_source": {
                            "message": "Error: \"invalid json\" at line 10\nStack trace:\n\t- func1()",
                            "pod_name": "test-pod",
                            "namespace": "test",
                            "container_name": "test",
                            "pod_id": "test-uuid"
                        }
                    }
                ]
            }
        }"#;

        let root: Root = serde_json::from_str(json).unwrap();
        let hits = root.hits.hits.unwrap();

        assert!(hits[0].source.message.contains("\"invalid json\""));
        assert!(hits[0].source.message.contains('\n'));
    }
}
