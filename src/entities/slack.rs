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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slack_new() {
        let slack = Slack::new("Test message".to_string(), 5);

        assert_eq!(slack.text, "Test message");
        assert_eq!(slack.blocks.len(), 1);
        assert_eq!(slack.blocks[0].type_, "section");
    }

    #[test]
    fn test_slack_serialization() {
        let slack = Slack::new("Error in pod".to_string(), 3);
        let json = serde_json::to_string(&slack).unwrap();

        assert!(json.contains("\"text\":\"Error in pod\""));
        assert!(json.contains("\"type\":\"section\""));
        assert!(json.contains("\"type\":\"mrkdwn\""));
        assert!(json.contains("*Frequency*"));
        assert!(json.contains("\"text\":\"3\""));
    }

    #[test]
    fn test_slack_block_structure() {
        let block = SlackBlock::new("Block text".to_string(), 10);

        assert_eq!(block.type_, "section");
        assert_eq!(block.text.type_, "mrkdwn");
        assert_eq!(block.text.text, "Block text");
        assert_eq!(block.fields.len(), 2);
        assert_eq!(block.fields[0].text, "*Frequency*");
        assert_eq!(block.fields[1].text, "10");
    }

    #[test]
    fn test_slack_frequency_one() {
        let slack = Slack::new("Single event".to_string(), 1);
        let json = serde_json::to_string(&slack).unwrap();

        assert!(json.contains("\"text\":\"1\""));
    }

    #[test]
    fn test_slack_large_frequency() {
        let slack = Slack::new("Many events".to_string(), 999999);
        let json = serde_json::to_string(&slack).unwrap();

        assert!(json.contains("\"text\":\"999999\""));
    }

    #[test]
    fn test_slack_special_characters_in_text() {
        let message = "Error: \"failed\" at line 10\nDetails: <script>alert('xss')</script>";
        let slack = Slack::new(message.to_string(), 1);
        let json = serde_json::to_string(&slack).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["text"].as_str().unwrap(), message);
    }

    #[test]
    fn test_slack_json_structure() {
        let slack = Slack::new("Test".to_string(), 1);
        let json = serde_json::to_string(&slack).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(parsed["text"].is_string());
        assert!(parsed["blocks"].is_array());
        assert_eq!(parsed["blocks"].as_array().unwrap().len(), 1);

        let block = &parsed["blocks"][0];
        assert_eq!(block["type"], "section");
        assert!(block["text"].is_object());
        assert!(block["fields"].is_array());
    }
}
