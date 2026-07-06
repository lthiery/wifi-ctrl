use super::*;

/// A vector of ScanResult, wrapped in an Arc. If more than one client is awaiting the result of a
/// scan, the result will be shared between them.
pub type ScanResults = Arc<Vec<ScanResult>>;

#[derive(Debug)]
/// Result from selecting a network, including a success or a specific failure (eg: incorect psk).
/// Timeout does not necessarily mean failure; it only means that we did not received a parseable response.
/// It could be that some valid message isn't being parsed by the library.
pub enum SelectResult {
    Success,
    WrongPsk,
    NotFound,
    AlreadyConnected,
}

use std::fmt;

impl fmt::Display for SelectResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            SelectResult::Success => "success",
            SelectResult::WrongPsk => "wrong_psk",
            SelectResult::NotFound => "network_not_found",
            SelectResult::AlreadyConnected => "already_connected",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug)]
pub(crate) enum RemoveNetwork {
    Id(usize),
    All,
}

#[derive(Debug)]
pub(crate) enum Request {
    Custom(String, oneshot::Sender<Result<String>>),
    Status(oneshot::Sender<Result<Status>>),
    Networks(oneshot::Sender<Result<Vec<NetworkResult>>>),
    Scan(oneshot::Sender<Result<ScanResults>>),
    AddNetwork(oneshot::Sender<Result<usize>>),
    SetNetwork(usize, SetNetwork, oneshot::Sender<Result>),
    SaveConfig(oneshot::Sender<Result>),
    ReloadConfig(oneshot::Sender<Result>),
    RemoveNetwork(RemoveNetwork, oneshot::Sender<Result>),
    SelectNetwork(usize, oneshot::Sender<Result<SelectResult>>),
    Shutdown,
}

impl ShutdownSignal for Request {
    fn is_shutdown(&self) -> bool {
        matches!(self, Request::Shutdown)
    }
}

#[derive(Debug)]
pub(crate) enum SetNetwork {
    Ssid(String),
    Bssid(Bssid),
    Psk(Psk),
    KeyMgmt(KeyMgmt),
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
        build_request: impl FnOnce(oneshot::Sender<Result<T>>) -> Request,
    ) -> Result<T> {
        let (response, request) = oneshot::channel();
        self.sender.send(build_request(response)).await?;
        request.await?
    }

    pub async fn send_custom(&self, custom: String) -> Result<String> {
        self.request(|response| Request::Custom(custom, response))
            .await
    }

    pub async fn get_scan(&self) -> Result<Arc<Vec<ScanResult>>> {
        self.request(Request::Scan).await
    }

    pub async fn get_networks(&self) -> Result<Vec<NetworkResult>> {
        self.request(Request::Networks).await
    }

    pub async fn get_status(&self) -> Result<Status> {
        self.request(Request::Status).await
    }

    pub async fn add_network(&self) -> Result<usize> {
        self.request(Request::AddNetwork).await
    }

    /// Set the network's pre-shared key. See [`Psk`] for the accepted forms.
    pub async fn set_network_psk(&self, network_id: usize, psk: Psk) -> Result {
        self.request(|response| Request::SetNetwork(network_id, SetNetwork::Psk(psk), response))
            .await
    }

    pub async fn set_network_ssid(&self, network_id: usize, ssid: String) -> Result {
        self.request(|response| Request::SetNetwork(network_id, SetNetwork::Ssid(ssid), response))
            .await
    }

    /// Pin the network to a specific access point by [`Bssid`].
    pub async fn set_network_bssid(&self, network_id: usize, bssid: Bssid) -> Result {
        self.request(|response| Request::SetNetwork(network_id, SetNetwork::Bssid(bssid), response))
            .await
    }

    /// Set the network's key management mode; see [`KeyMgmt`].
    pub async fn set_network_keymgmt(&self, network_id: usize, mgmt: KeyMgmt) -> Result {
        self.request(|response| {
            Request::SetNetwork(network_id, SetNetwork::KeyMgmt(mgmt), response)
        })
        .await
    }

    pub async fn save_config(&self) -> Result {
        self.request(Request::SaveConfig).await
    }

    pub async fn reload_config(&self) -> Result {
        self.request(Request::ReloadConfig).await
    }

    pub async fn remove_network(&self, id: usize) -> Result {
        self.request(|response| Request::RemoveNetwork(RemoveNetwork::Id(id), response))
            .await
    }

    pub async fn remove_all_networks(&self) -> Result {
        self.request(|response| Request::RemoveNetwork(RemoveNetwork::All, response))
            .await
    }

    pub async fn select_network(&self, network_id: usize) -> Result<SelectResult> {
        self.request(|response| Request::SelectNetwork(network_id, response))
            .await
    }

    pub async fn shutdown(&self) -> Result {
        self.sender.send(Request::Shutdown).await?;
        Ok(())
    }
}

/// Broadcast events are unexpected, such as losing connection to the host network.
#[derive(Debug, Clone)]
pub enum Broadcast {
    Connected,
    Disconnected,
    NetworkNotFound,
    WrongPsk,
    Ready,
    Unknown(String),
}

/// Channel for broadcasting events. Subscribing to this channel is equivalent to
/// "wpa_ctrl_attach". Can be temporarily silenced using broadcast::Receiver's unsubscribe
pub type BroadcastReceiver = broadcast::Receiver<Broadcast>;
