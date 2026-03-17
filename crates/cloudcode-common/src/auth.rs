use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum AuthMethod {
    ApiKey {
        key: String,
    },
    #[serde(rename = "oauth")]
    OAuth {
        token: String,
    },
}
