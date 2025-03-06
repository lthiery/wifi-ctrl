use super::*;

#[derive(Debug)]
pub(crate) enum Request {
    Custom(String, oneshot::Sender<Result<String>>),
    Status(oneshot::Sender<Result<Status>>),
    Config(oneshot::Sender<Result<Config>>),
    Enable(oneshot::Sender<Result>),
    Disable(oneshot::Sender<Result>),
    SetValue(String, String, oneshot::Sender<Result>),
    Shutdown,
}

impl ShutdownSignal for Request {
    fn is_shutdown(&self) -> bool {
        matches!(self, Request::Shutdown)
    }
}

#[derive(Clone)]
/// Request client wraps the request events, awaiting oneshot channels when appropriate
pub struct RequestClient {
    sender: mpsc::Sender<Request>,
}

impl RequestClient {
    pub(crate) fn new(sender: mpsc::Sender<Request>) -> RequestClient {
        RequestClient { sender }
    }

    pub async fn send_custom(&self, custom: String) -> Result<String> {
        let (response, request) = oneshot::channel();
        self.sender.send(Request::Custom(custom, response)).await?;
        request.await?
    }

    pub async fn get_status(&self) -> Result<Status> {
        let (response, request) = oneshot::channel();
        self.sender.send(Request::Status(response)).await?;
        request.await?
    }

    pub async fn get_config(&self) -> Result<Config> {
        let (response, request) = oneshot::channel();
        self.sender.send(Request::Config(response)).await?;
        request.await?
    }

    pub async fn enable(&self) -> Result {
        let (response, request) = oneshot::channel();
        self.sender.send(Request::Enable(response)).await?;
        request.await?
    }

    pub async fn disable(&self) -> Result {
        let (response, request) = oneshot::channel();
        self.sender.send(Request::Disable(response)).await?;
        request.await?
    }

    pub async fn set_value(&self, key: &str, value: &str) -> Result {
        let (response, request) = oneshot::channel();
        self.sender
            .send(Request::SetValue(key.into(), value.into(), response))
            .await?;
        request.await?
    }

    pub async fn shutdown(&self) -> Result {
        Ok(self.sender.send(Request::Shutdown).await?)
    }
}

#[derive(Debug, Clone)]
/// Broadcast events, such as a client disconnecting or connecting, may happen at any time.
pub enum Broadcast {
    Ready,
    Connected(String),
    Disconnected(String),
    UnknownEvent(String),
}

/// Channel for broadcasting events.
pub type BroadcastReceiver = broadcast::Receiver<Broadcast>;
