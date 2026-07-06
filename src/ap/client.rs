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

    async fn request<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<Result<T>>) -> Request,
    ) -> Result<T> {
        let (response, request) = oneshot::channel();
        self.sender.send(make(response)).await?;
        request.await?
    }

    pub async fn send_custom(&self, custom: String) -> Result<String> {
        self.request(|response| Request::Custom(custom, response))
            .await
    }

    pub async fn get_status(&self) -> Result<Status> {
        self.request(Request::Status).await
    }

    pub async fn get_config(&self) -> Result<Config> {
        self.request(Request::Config).await
    }

    pub async fn enable(&self) -> Result {
        self.request(Request::Enable).await
    }

    pub async fn disable(&self) -> Result {
        self.request(Request::Disable).await
    }

    pub async fn set_value(&self, key: &str, value: &str) -> Result {
        self.request(|response| Request::SetValue(key.into(), value.into(), response))
            .await
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
