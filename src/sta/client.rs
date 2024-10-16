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
    PendingSelect,
    InvalidNetworkId,
    Timeout,
    AlreadyConnected,
}

use std::fmt;

impl fmt::Display for SelectResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            SelectResult::Success => "success",
            SelectResult::WrongPsk => "wrong_psk",
            SelectResult::NotFound => "network_not_found",
            SelectResult::PendingSelect => "select_already_pending",
            SelectResult::InvalidNetworkId => "invalid_network_id",
            SelectResult::Timeout => "select_timeout",
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
    SelectTimeout,
}

impl ShutdownSignal for Request {
    fn is_shutdown(&self) -> bool {
        matches!(self, Request::Shutdown)
    }
    fn inform_of_shutdown(self) {
        match self {
            Request::Custom(_, response) => {
                let _ = response.send(Err(error::Error::StartupAborted));
            }
            Request::Status(response) => {
                let _ = response.send(Err(error::Error::StartupAborted));
            }
            Request::Networks(response) => {
                let _ = response.send(Err(error::Error::StartupAborted));
            }
            Request::Scan(response) => {
                let _ = response.send(Err(error::Error::StartupAborted));
            }
            Request::AddNetwork(response) => {
                let _ = response.send(Err(error::Error::StartupAborted));
            }
            Request::SetNetwork(_, _, response) => {
                let _ = response.send(Err(error::Error::StartupAborted));
            }
            Request::SaveConfig(response) => {
                let _ = response.send(Err(error::Error::StartupAborted));
            }
            Request::ReloadConfig(response) => {
                let _ = response.send(Err(error::Error::StartupAborted));
            }
            Request::RemoveNetwork(_, response) => {
                let _ = response.send(Err(error::Error::StartupAborted));
            }
            Request::SelectNetwork(_, response) => {
                let _ = response.send(Err(error::Error::StartupAborted));
            }
            Request::Shutdown => {}
            Request::SelectTimeout => {}
        }
    }
}

#[derive(Debug)]
pub(crate) enum SetNetwork {
    Ssid(String),
    Bssid(String),
    Psk(String),
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

    async fn send_request(&self, request: Request) -> Result {
        self.sender
            .send(request)
            .await
            .map_err(|_| error::Error::WifiStationRequestChannelClosed)?;
        Ok(())
    }

    pub async fn send_custom(&self, custom: String) -> Result<String> {
        let (response, request) = oneshot::channel();
        self.send_request(Request::Custom(custom, response)).await?;
        request.await?
    }

    pub async fn get_scan(&self) -> Result<Arc<Vec<ScanResult>>> {
        let (response, request) = oneshot::channel();
        self.send_request(Request::Scan(response)).await?;
        request.await?
    }

    pub async fn get_networks(&self) -> Result<Vec<NetworkResult>> {
        let (response, request) = oneshot::channel();
        self.send_request(Request::Networks(response)).await?;
        request.await?
    }

    pub async fn get_status(&self) -> Result<Status> {
        let (response, request) = oneshot::channel();
        self.send_request(Request::Status(response)).await?;
        request.await?
    }

    pub async fn add_network(&self) -> Result<usize> {
        let (response, request) = oneshot::channel();
        self.send_request(Request::AddNetwork(response)).await?;
        request.await?
    }

    pub async fn set_network_psk(&self, network_id: usize, psk: String) -> Result {
        let (response, request) = oneshot::channel();
        self.send_request(Request::SetNetwork(
            network_id,
            SetNetwork::Psk(psk),
            response,
        ))
        .await?;
        request.await?
    }

    pub async fn set_network_ssid(&self, network_id: usize, ssid: String) -> Result {
        let (response, request) = oneshot::channel();
        self.send_request(Request::SetNetwork(
            network_id,
            SetNetwork::Ssid(ssid),
            response,
        ))
        .await?;
        request.await?
    }

    pub async fn set_network_bssid(&self, network_id: usize, bssid: String) -> Result {
        let (response, request) = oneshot::channel();
        self.send_request(Request::SetNetwork(
            network_id,
            SetNetwork::Bssid(bssid),
            response,
        ))
        .await?;
        request.await?
    }

    pub async fn set_network_keymgmt(&self, network_id: usize, mgmt: KeyMgmt) -> Result {
        let (response, request) = oneshot::channel();
        self.send_request(Request::SetNetwork(
            network_id,
            SetNetwork::KeyMgmt(mgmt),
            response,
        ))
        .await?;
        request.await?
    }

    pub async fn save_config(&self) -> Result {
        let (response, request) = oneshot::channel();
        self.send_request(Request::SaveConfig(response)).await?;
        request.await?
    }

    pub async fn reload_config(&self) -> Result {
        let (response, request) = oneshot::channel();
        self.send_request(Request::ReloadConfig(response)).await?;
        request.await?
    }

    pub async fn remove_network(&self, id: usize) -> Result {
        let (response, request) = oneshot::channel();
        self.send_request(Request::RemoveNetwork(RemoveNetwork::Id(id), response))
            .await?;
        request.await?
    }

    pub async fn remove_all_networks(&self) -> Result {
        let (response, request) = oneshot::channel();
        self.send_request(Request::RemoveNetwork(RemoveNetwork::All, response))
            .await?;
        request.await?
    }

    pub async fn select_network(&self, network_id: usize) -> Result<SelectResult> {
        let (response, request) = oneshot::channel();
        self.send_request(Request::SelectNetwork(network_id, response))
            .await?;
        request.await?
    }

    pub async fn shutdown(&self) -> Result {
        self.send_request(Request::Shutdown).await?;
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
