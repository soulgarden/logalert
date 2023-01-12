use serde_derive::Deserialize;

#[derive(Deserialize)]
pub struct Source {
    pub message: String,
    #[serde(rename(deserialize = "@timestamp"))]
    pub timestamp: String,
    pub pod_name: String,
    pub namespace: String,
    pub container_name: String,
    pub pod_id: String,
}

#[derive(Deserialize)]
pub struct Struct {
    #[serde(rename(deserialize = "_index"))]
    pub index: String,
    pub _type: String,
    #[serde(rename(deserialize = "_id"))]
    pub id: String,
    #[serde(rename(deserialize = "_score"))]
    pub score: Option<f64>,
    #[serde(rename(deserialize = "_source"))]
    pub source: Source,
}

#[derive(Deserialize)]
pub struct Total {
    pub value: i64,
    pub relation: String,
}

#[derive(Deserialize)]
pub struct Hits {
    pub total: Total,
    pub max_score: Option<f64>,
    pub hits: Vec<Struct>,
}

#[derive(Deserialize)]
pub struct Shards {
    pub total: i64,
    pub successful: i64,
    pub skipped: i64,
    pub failed: i64,
}

#[derive(Deserialize)]
pub struct Root {
    pub took: i64,
    pub timed_out: bool,
    #[serde(rename(deserialize = "_shards"))]
    pub shards: Shards,
    pub hits: Hits,
}
