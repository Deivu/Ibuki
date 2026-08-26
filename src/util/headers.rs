use axum::http::{HeaderMap, HeaderValue};
use rand_agents::user_agent;
use reqwest::header::{
    ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, CACHE_CONTROL, CONNECTION, DNT, InvalidHeaderValue,
    PRAGMA, USER_AGENT,
};

pub fn generate_headers() -> Result<HeaderMap<HeaderValue>, InvalidHeaderValue> {
    let mut headers = HeaderMap::new();

    let user_agent = user_agent();

    headers.insert(CONNECTION, "keep-alive".parse()?);
    headers.insert(CACHE_CONTROL, "no-cache".parse()?);
    headers.insert(ACCEPT, "*/*".parse()?);
    headers.insert(USER_AGENT, user_agent.parse()?);
    headers.insert(DNT, "1".parse()?);
    headers.insert(ACCEPT_ENCODING, "gzip, deflate, br".parse()?);
    headers.insert(ACCEPT_LANGUAGE, "en-US;q=0.5,en;q=0.3".parse()?);
    headers.insert(PRAGMA, "no-cache".parse()?);

    Ok(headers)
}
