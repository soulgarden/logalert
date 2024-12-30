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
pub struct Shards {}

#[derive(Deserialize)]
pub struct Root {
    pub hits: Hits,
}
