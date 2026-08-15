use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeezerConfig {
    pub decrypt_key: String,
    pub arl: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeConfig {
    pub use_po_token: Option<bool>,
    pub use_oauth: Option<bool>,
    pub cookies: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpConfig {}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub port: u16,
    pub address: String,
    pub authorization: String,
    pub player_update_secs: Option<u8>,
    pub status_update_secs: Option<u8>,
    pub deezer_config: Option<DeezerConfig>,
    pub youtube_config: Option<YoutubeConfig>,
    pub http_config: Option<HttpConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}

impl Config {
    pub fn new() -> Self {
        let config = fs::read_to_string("./config.json").expect("Missing ./config.json");
        serde_json::from_str::<Config>(&config).unwrap()
    }
}
