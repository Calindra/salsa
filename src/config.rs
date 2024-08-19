#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub http_address: String,
    pub http_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

impl Config {
    pub fn new() -> Self {
        Self {
            http_address: String::from("127.0.0.1"),
            http_port: 5005,
        }
    }
}