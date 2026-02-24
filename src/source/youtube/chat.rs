use reqwest::Client;
use serde_json::Value;


use super::api::InnertubeApi;
use crate::util::errors::ResolverError;

pub struct LiveChat {
    http: Client,
    api: InnertubeApi, // Re-use api logic? Or independent?
}

impl LiveChat {
    pub fn new(http: Client) -> Self {
        Self {
            http,
            api: InnertubeApi::new(),
        }
    }

    // Placeholder for live chat polling
    pub async fn poll(&self, _continuation: &str) -> Result<Value, ResolverError> {
        // Need to implement the "get_live_chat" endpoint logic
        // self.api.make_request(...)
        Err(ResolverError::Custom("LiveChat not implemented".to_string()))
    }
}
