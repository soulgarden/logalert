use serde_derive::Serialize;

#[derive(Serialize)]
pub struct Slack {
    pub text: String,
}
