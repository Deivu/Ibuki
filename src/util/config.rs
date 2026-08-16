use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub port: u16,
    pub address: String,
    pub authorization: String,
    pub player_update_secs: Option<u8>,
    pub status_update_secs: Option<u8>,
    pub plugins: Value,
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
