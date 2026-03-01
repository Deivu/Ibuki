use serde::Deserialize;

pub mod endpoints;
pub mod global;
pub mod youtube;

#[derive(Deserialize, Debug)]
pub struct PlayerMethodsPath {
    pub session_id: String,
    pub guild_id: u64,
}

#[derive(Deserialize, Debug)]
pub struct SessionMethodsPath {
    pub session_id: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlayerUpdateQuery {
    pub no_replace: Option<bool>,
}

#[derive(Deserialize, Debug)]
pub struct DecodeQueryString {
    pub track: String,
}

#[derive(Deserialize, Debug)]
pub struct EncodeQueryString {
    pub identifier: String,
}
