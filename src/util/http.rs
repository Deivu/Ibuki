use reqwest::{Client, Method, RequestBuilder, StatusCode};
use serde_json::Value;
use std::time::Duration;
use bytes::Bytes;
use crate::util::errors::ResolverError;

pub struct HttpOptions {
    pub method: Method,
    pub headers: reqwest::header::HeaderMap,
    pub body: Option<Bytes>,
    pub timeout: Option<Duration>,
    pub stream_only: bool,
}

impl Default for HttpOptions {
    fn default() -> Self {
        Self {
            method: Method::GET,
            headers: reqwest::header::HeaderMap::new(),
            body: None,
            timeout: Some(Duration::from_secs(30)),
            stream_only: false,
        }
    }
}

pub struct HttpResponse {
    pub status: StatusCode,
    pub headers: reqwest::header::HeaderMap,
    pub body: Option<Bytes>, // None if stream_only
    pub text: Option<String>,
    pub json: Option<Value>,
}

pub async fn http1_make_request(url: &str, client: &Client, options: HttpOptions) -> Result<HttpResponse, ResolverError> {
    let mut req = client.request(options.method, url);
    req = req.headers(options.headers);
    if let Some(body) = options.body {
        req = req.body(body);
    }
    if let Some(timeout) = options.timeout {
        req = req.timeout(timeout);
    }
    
    let res = req.send().await.map_err(ResolverError::Reqwest)?;
    let status = res.status();
    let headers = res.headers().clone();
    
    if options.stream_only {
        let body = res.bytes().await.map_err(ResolverError::Reqwest)?;
        return Ok(HttpResponse {
            status,
            headers,
            body: Some(body),
            text: None,
            json: None,
        });
    }
    
    let body_bytes = res.bytes().await.map_err(ResolverError::Reqwest)?;
    let text = String::from_utf8_lossy(&body_bytes).to_string();
    let json: Option<Value> = serde_json::from_str(&text).ok();
    
    Ok(HttpResponse {
        status,
        headers,
        body: Some(body_bytes),
        text: Some(text),
        json,
    })
}
