use serde_derive::Serialize;

#[derive(Serialize)]
pub struct Slack {
    pub text: String,
    pub blocks: Vec<SlackBlock>,
}

impl Slack {
    pub fn new(text: String, frequency: usize) -> Self {
        Slack {
            text: text.clone(),
            blocks: vec![SlackBlock::new(text, frequency)],
        }
    }
}

#[derive(Serialize)]
pub struct SlackBlock {
    #[serde(rename(serialize = "type"))]
    pub type_: String,
    pub text: SlackBlockText,
    pub fields: Vec<SlackBlockField>,
}

impl SlackBlock {
    pub fn new(text: String, frequency: usize) -> Self {
        SlackBlock {
            type_: "section".to_string(),
            text: SlackBlockText::new(text),
            fields: vec![
                SlackBlockField::new("*Frequency*".to_string()),
                SlackBlockField::new(frequency.to_string()),
            ],
        }
    }
}

#[derive(Serialize)]
pub struct SlackBlockText {
    #[serde(rename(serialize = "type"))]
    pub type_: String,
    pub text: String,
}

impl SlackBlockText {
    pub fn new(text: String) -> Self {
        SlackBlockText {
            type_: "mrkdwn".to_string(),
            text,
        }
    }
}

#[derive(Serialize)]
pub struct SlackBlockField {
    #[serde(rename(serialize = "type"))]
    pub type_: String,
    pub text: String,
}

impl SlackBlockField {
    pub fn new(text: String) -> Self {
        SlackBlockField {
            type_: "mrkdwn".to_string(),
            text,
        }
    }
}
