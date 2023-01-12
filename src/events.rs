#[derive(Clone, Debug)]
pub struct Event {
    pub id: String,
    pub message: String,
    pub timestamp: String,
    pub meta: Meta,
}

#[derive(Clone, Debug, Default)]
pub struct Meta {
    pub pod_name: String,
    pub namespace: String,
    pub container_name: String,
    pub pod_id: String,
}

impl Meta {
    pub fn new(
        pod_name: String,
        namespace: String,
        container_name: String,
        pod_id: String,
    ) -> Self {
        Self {
            pod_name,
            namespace,
            container_name,
            pod_id,
        }
    }
}

impl Event {
    pub fn new(id: String, message: String, timestamp: String, meta: Meta) -> Self {
        Self {
            id,
            message,
            timestamp,
            meta: Meta {
                pod_name: meta.pod_name,
                namespace: meta.namespace,
                container_name: meta.container_name,
                pod_id: meta.pod_id,
            },
        }
    }
}
