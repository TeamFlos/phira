use serde::{Deserialize, Serialize};

/// Settings for the game client
#[derive(Debug, Serialize, Deserialize)]
pub struct ClientSettings {
    pub anys_gateway: String,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            anys_gateway: "https://anys.mivik.moe".to_string(),
        }
    }
}
