use axum::http::{HeaderMap, HeaderValue};
use rand_agents::user_agent;
use reqwest::header::{
    ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, CACHE_CONTROL, CONNECTION, DNT, InvalidHeaderValue,
    PRAGMA, REFERER, USER_AGENT,
};

pub fn generate_headers() -> Result<HeaderMap<HeaderValue>, InvalidHeaderValue> {
    let mut headers = HeaderMap::new();

    let user_agent = user_agent();

    headers.insert(CONNECTION, "keep-alive".parse()?);
    headers.insert(CACHE_CONTROL, "no-cache".parse()?);
    headers.insert(ACCEPT, "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8".parse()?);
    headers.insert(USER_AGENT, user_agent.parse()?);
    headers.insert(DNT, "1".parse()?);
    headers.insert(ACCEPT_ENCODING, "gzip, deflate, br, zstd".parse()?);
    headers.insert(ACCEPT_LANGUAGE, "en-US,en;q=0.9,en-GB;q=0.8".parse()?);
    headers.insert(PRAGMA, "no-cache".parse()?);
    headers.insert(REFERER, "https://www.deezer.com/".parse()?);
    headers.insert("Sec-Fetch-Dest", "document".parse()?);
    headers.insert("Sec-Fetch-Mode", "navigate".parse()?);
    headers.insert("Sec-Fetch-Site", "none".parse()?);
    headers.insert("Sec-Fetch-User", "?1".parse()?);
    headers.insert("Upgrade-Insecure-Requests", "1".parse()?);
    headers.insert("Sec-Ch-Ua", "\"Chromium\";v=\"122\", \"Not(A:Brand\";v=\"24\", \"Google Chrome\";v=\"122\"".parse()?);
    headers.insert("Sec-Ch-Ua-Mobile", "?0".parse()?);
    headers.insert("Sec-Ch-Ua-Platform", "\"Windows\"".parse()?);

    Ok(headers)
}
