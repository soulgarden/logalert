pub struct Message {
    pub frequency: usize,
    pub text: String,
}

impl Message {
    pub fn new(text: String, frequency: usize) -> Self {
        Self { text, frequency }
    }
}
