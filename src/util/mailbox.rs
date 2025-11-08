use async_trait::async_trait;
use flume::{Receiver, Sender};
use tokio::sync::oneshot;

#[async_trait]
pub trait AsyncInboxHandler<'a, T, R>
where
    T: Send + 'static,
    R: Send + 'static,
{
    async fn handle(&mut self, envelope: Envelope<T, R>);
}

pub enum Envelope<T, R>
where
    T: Send + 'static,
    R: Send + 'static,
{
    Message {
        data: Option<T>,
    },
    Request {
        data: Option<T>,
        tx: oneshot::Sender<R>,
    },
}

impl<T, R> Envelope<T, R>
where
    T: Send + 'static,
    R: Send + 'static,
{
    pub fn new_message(data: T) -> Self {
        Self::Message { data: Some(data) }
    }

    pub fn new_request(data: T, tx: oneshot::Sender<R>) -> Self {
        Self::Request {
            data: Some(data),
            tx,
        }
    }

    pub fn take_data(&mut self) -> Option<T> {
        match self {
            Self::Message { data } => data.take(),
            Self::Request { data, .. } => data.take(),
        }
    }

    pub fn is_request(&self) -> bool {
        matches!(self, Self::Request { .. })
    }

    pub fn reply(self, response: R) {
        let Self::Request { tx, .. } = self else {
            return;
        };
        let _ = tx.send(response);
    }
}

pub struct Inbox<T, R>
where
    T: Send + 'static,
    R: Send + 'static,
{
    rx: Receiver<Envelope<T, R>>,
}

impl<T, R> Inbox<T, R>
where
    T: Send + 'static,
    R: Send + 'static,
{
    pub fn new(rx: Receiver<Envelope<T, R>>) -> Self {
        Self { rx }
    }

    pub async fn run<H>(&self, mut handler: H)
    where
        H: for<'a> AsyncInboxHandler<'a, T, R>,
    {
        while let Ok(envelope) = self.rx.recv_async().await {
            handler.handle(envelope).await;
        }
    }
}

#[derive(Clone)]
pub struct Outbox<T, R>
where
    T: Send + 'static,
    R: Send + 'static,
{
    tx: Sender<Envelope<T, R>>,
}

impl<T, R> Outbox<T, R>
where
    T: Send + 'static,
    R: Send + 'static,
{
    pub fn new(tx: Sender<Envelope<T, R>>) -> Self {
        Self { tx }
    }

    pub async fn send_message(&self, msg: T) -> bool {
        self.tx.send_async(Envelope::new_message(msg)).await.is_ok()
    }

    pub async fn send_request(&self, msg: T) -> oneshot::Receiver<R> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send_async(Envelope::new_request(msg, tx)).await;
        rx
    }
}

pub fn create_mailbox<T, R>() -> (Outbox<T, R>, Inbox<T, R>)
where
    T: Send + 'static,
    R: Send + 'static,
{
    let (tx, rx) = flume::unbounded();
    (Outbox::new(tx), Inbox::new(rx))
}
