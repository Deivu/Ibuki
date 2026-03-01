use async_trait::async_trait;
use reqwest::{
    header::{HeaderMap, CONTENT_LENGTH, CONTENT_TYPE, RETRY_AFTER},
    Client,
};
use songbird::input::{AsyncAdapterStream, AsyncMediaSource, AudioStream, AudioStreamError, Compose, Input};
use std::{
    io::{Error as IoError, ErrorKind as IoErrorKind, Result as IoResult, SeekFrom},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use pin_project::pin_project;
use symphonia::core::{io::MediaSource, probe::Hint};
use tokio::io::{AsyncRead, AsyncSeek, ReadBuf};
use tokio_util::io::StreamReader;
use futures::TryStreamExt;

#[derive(Clone, Debug)]
pub struct YoutubeHttpStream {
    pub client: Client,
    pub request: String,
    pub headers: HeaderMap,
    pub content_length: Option<u64>,
}

impl YoutubeHttpStream {
    pub fn new(client: Client, request: String, headers: HeaderMap) -> Self {
        Self {
            client,
            request,
            headers,
            content_length: None,
        }
    }

    async fn create_stream(
        &mut self,
        offset: Option<u64>,
    ) -> Result<(ActiveStream, Option<Hint>), AudioStreamError> {
        let mut req_url = self.request.clone();
        if let Some(off) = offset {
            let max_val = self.content_length.unwrap_or(off + 11862014); // 11MB buffer fallback
            let range_str = format!("&range={}-{}", off, max_val);
            
            if req_url.contains('?') {
                req_url.push_str(&range_str);
            } else {
                req_url.push_str(&range_str.replace("&range=", "?range="));
            }
        }

        let resp = self.client
            .get(&req_url)
            .headers(self.headers.clone())
            .send()
            .await
            .map_err(|e| AudioStreamError::Fail(Box::new(e)))?;

        if !resp.status().is_success() {
            let msg: Box<dyn std::error::Error + Send + Sync + 'static> =
                format!("failed with http status code: {}", resp.status()).into();
            return Err(AudioStreamError::Fail(msg));
        }

        if let Some(t) = resp.headers().get(RETRY_AFTER) {
            t.to_str()
                .map_err(|_| {
                    let msg: Box<dyn std::error::Error + Send + Sync + 'static> =
                        "Retry-after field contained non-ASCII data.".into();
                    AudioStreamError::Fail(msg)
                })
                .and_then(|str_text| {
                    str_text.parse().map_err(|_| {
                        let msg: Box<dyn std::error::Error + Send + Sync + 'static> =
                            "Retry-after field was non-numeric.".into();
                        AudioStreamError::Fail(msg)
                    })
                })
                .and_then(|t| Err(AudioStreamError::RetryIn(Duration::from_secs(t))))
        } else {
            let headers = resp.headers();

            let hint = headers
                .get(CONTENT_TYPE)
                .and_then(|val| val.to_str().ok())
                .map(|val| {
                    let mut out = Hint::default();
                    out.mime_type(val);
                    out
                });

            let len = headers
                .get(CONTENT_LENGTH)
                .and_then(|val| val.to_str().ok())
                .and_then(|val| val.parse().ok());

            if self.content_length.is_none() {
                self.content_length = len;
            }

            let stream = Box::new(StreamReader::new(
                resp.bytes_stream().map_err(IoError::other),
            ));

            let input = ActiveStream {
                stream,
                len,
                resume: Some(self.clone()),
            };

            Ok((input, hint))
        }
    }
}

#[pin_project]
pub struct ActiveStream {
    #[pin]
    stream: Box<dyn AsyncRead + Send + Sync + Unpin>,
    len: Option<u64>,
    resume: Option<YoutubeHttpStream>,
}

impl AsyncRead for ActiveStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<IoResult<()>> {
        AsyncRead::poll_read(self.project().stream, cx, buf)
    }
}

impl AsyncSeek for ActiveStream {
    fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> IoResult<()> {
        Err(IoErrorKind::Unsupported.into())
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<IoResult<u64>> {
        unreachable!()
    }
}

#[async_trait]
impl AsyncMediaSource for ActiveStream {
    fn is_seekable(&self) -> bool {
        false
    }

    async fn byte_len(&self) -> Option<u64> {
        self.len
    }

    async fn try_resume(
        &mut self,
        offset: u64,
    ) -> Result<Box<dyn AsyncMediaSource>, AudioStreamError> {
        if let Some(resume) = &mut self.resume {
            resume
                .create_stream(Some(offset))
                .await
                .map(|a| Box::new(a.0) as Box<dyn AsyncMediaSource>)
        } else {
            Err(AudioStreamError::Unsupported)
        }
    }
}

#[async_trait]
impl Compose for YoutubeHttpStream {
    fn create(&mut self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        Err(AudioStreamError::Unsupported)
    }

    async fn create_async(
        &mut self,
    ) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        self.create_stream(None).await.map(|(input, hint)| {
            let stream = AsyncAdapterStream::new(Box::new(input), 64 * 1024);

            AudioStream {
                input: Box::new(stream) as Box<dyn MediaSource>,
                hint,
            }
        })
    }

    fn should_create_async(&self) -> bool {
        true
    }
}

impl From<YoutubeHttpStream> for Input {
    fn from(val: YoutubeHttpStream) -> Self {
        Input::Lazy(Box::new(val))
    }
}
