use core::str;

use super::*;

use tokio::time::Duration;

mod types;
pub use types::*;

mod client;
pub use client::*;

mod setup;
pub use setup::*;

mod event_socket;
use event_socket::*;

const PATH_DEFAULT_SERVER: &str = "/var/run/wpa_supplicant/wlan2";

/// Instance that runs the Wifi process
pub struct WifiStation {
    /// Path to the socket
    socket_path: std::path::PathBuf,
    /// Channel for receiving requests
    request_receiver: mpsc::Receiver<Request>,
    #[allow(unused)]
    /// Channel for broadcasting alerts
    broadcast_sender: broadcast::Sender<Broadcast>,
    /// Channel for sending requests to itself
    self_sender: mpsc::Sender<Request>,
    /// Timeout duration in case no valid select response is received
    select_timeout: Duration,
}

impl WifiStation {
    pub async fn run(&mut self) -> SocketResult {
        info!("Starting Wifi Station process");
        let (socket_handle, mut deferred_requests) = SocketHandle::open(
            &self.socket_path,
            "mapper_wpa_ctrl_sync.sock",
            &mut self.request_receiver,
        )
        .await?;
        // We start up a separate socket for receiving the "unexpected" events that
        // gets forwarded to us via the unsolicited_receiver
        let (next_deferred_requests, unsolicited) =
            EventSocket::new(&self.socket_path, &mut self.request_receiver).await?;
        deferred_requests.extend(next_deferred_requests);
        for request in deferred_requests {
            let _ = self.self_sender.send(request).await;
        }
        self.broadcast(Broadcast::Ready);
        self.run_internal(unsolicited, socket_handle).await
    }

    fn broadcast(&self, event: Broadcast) {
        if self.broadcast_sender.send(event).is_err() {
            debug!("broadcast listener closed")
        }
    }

    async fn run_internal(
        &mut self,
        mut unsolicited: EventSocket,
        mut socket_handle: SocketHandle<10240>,
    ) -> SocketResult {
        // We will collect scan requests and batch respond to them when results are ready
        let mut scan_requests = Vec::new();
        let mut select_request = None;
        loop {
            enum EventOrRequest {
                Event(Event),
                Request(Option<Request>),
            }

            let event_or_request = tokio::select!(
                unsolicited_msg = unsolicited.recv() => {
                    EventOrRequest::Event(unsolicited_msg?)
                },
                request = self.request_receiver.recv() => {
                    EventOrRequest::Request(request)
                },
            );

            match event_or_request {
                EventOrRequest::Event(unsolicited_msg) => {
                    debug!("Unsolicited event: {unsolicited_msg:?}");
                    self.handle_event(unsolicited_msg, &mut scan_requests, &mut select_request)
                        .await
                }
                EventOrRequest::Request(request) => match request {
                    Some(Request::Shutdown) => return Ok(()),
                    Some(request) => {
                        self.handle_request(
                            &mut socket_handle,
                            request,
                            &mut scan_requests,
                            &mut select_request,
                        )
                        .await?;
                    }
                    None => return Err(error::SocketError::ClientChannelClosed),
                },
            }
        }
    }

    async fn handle_event(
        &mut self,
        event: Event,
        scan_requests: &mut Vec<oneshot::Sender<Result<Arc<Vec<ScanResult>>>>>,
        select_request: &mut Option<SelectRequest>,
    ) {
        match event {
            Event::ScanComplete => {
                let _ = self.self_sender.send(Request::ScanResults).await;
            }
            Event::ScanFailed => {
                while let Some(scan_request) = scan_requests.pop() {
                    let _ = scan_request.send(Err(error::ClientError::Failed));
                }
            }
            Event::Connected => {
                self.broadcast(Broadcast::Connected);
                if let Some(sender) = select_request.take() {
                    sender.send(Ok(SelectResult::Success));
                }
            }
            Event::Disconnected => {
                self.broadcast(Broadcast::Disconnected);
            }
            Event::NetworkNotFound => {
                self.broadcast(Broadcast::NetworkNotFound);
                if let Some(sender) = select_request.take() {
                    sender.send(Ok(SelectResult::NotFound));
                }
            }
            Event::WrongPsk => {
                self.broadcast(Broadcast::WrongPsk);
                if let Some(sender) = select_request.take() {
                    sender.send(Ok(SelectResult::WrongPsk));
                }
            }
            Event::Unknown(msg) => {
                self.broadcast(Broadcast::Unknown(msg));
            }
        }
    }

    async fn get_status<const N: usize>(
        socket_handle: &mut SocketHandle<N>,
    ) -> SocketResult<Result<Status>> {
        socket_handle.request("STATUS", parse_status).await
    }

    async fn handle_request<const N: usize>(
        &self,
        socket_handle: &mut SocketHandle<N>,
        request: Request,
        scan_requests: &mut Vec<oneshot::Sender<Result<Arc<Vec<ScanResult>>>>>,
        select_request: &mut Option<SelectRequest>,
    ) -> SocketResult {
        debug!("Handling request: {request:?}");
        match request {
            Request::Custom(custom, response_channel) => {
                let data_str = socket_handle.request(&custom, TryInto::try_into).await?;
                debug!("Custom request response: {data_str:?}");
                let _ = response_channel.send(data_str);
            }
            Request::SelectTimeout => {
                if let Some(sender) = select_request.take() {
                    sender.send(Ok(SelectResult::Timeout));
                }
            }
            Request::Scan(response_channel) => {
                match socket_handle.command(b"SCAN").await? {
                    Ok(_) => {
                        scan_requests.push(response_channel);
                    }
                    Err(e) => {
                        let _ = response_channel.send(Err(e));
                    }
                };
            }
            Request::ScanResults => {
                let scan_results = socket_handle
                    .request("SCAN_RESULTS", ScanResult::vec_from_str)
                    .await?;
                while let Some(scan_request) = scan_requests.pop() {
                    let _ = scan_request.send(scan_results.clone());
                }
            }
            Request::Networks(response_channel) => {
                let network_list = NetworkResult::request_results(socket_handle).await?;
                let _ = response_channel.send(network_list);
            }
            Request::Status(response_channel) => {
                let status = Self::get_status(socket_handle).await?;
                let _ = response_channel.send(status);
            }
            Request::AddNetwork(response_channel) => {
                let network_id = socket_handle
                    .request("ADD_NETWORK", usize::from_str)
                    .await?;
                debug!("wpa_ctrl created network {network_id:?}");
                let _ = response_channel.send(network_id);
            }
            Request::SetNetwork(id, param, response) => {
                let cmd = format!(
                    "SET_NETWORK {id} {}",
                    match param {
                        SetNetwork::Ssid(ssid) => format!("ssid {}", conf_escape(&ssid)),
                        SetNetwork::Bssid(bssid) => format!("bssid {}", conf_escape(&bssid)),
                        SetNetwork::Psk(psk) => format!("psk {}", conf_escape(&psk)),
                        SetNetwork::KeyMgmt(mgmt) => format!("key_mgmt {}", mgmt),
                    }
                );
                debug!("wpa_ctrl {cmd:?}");
                let bytes = cmd.into_bytes();
                let _ = response.send(socket_handle.command(&bytes).await?);
            }
            Request::SaveConfig(response) => {
                debug!("wpa_ctrl config saved");
                let _ = response.send(socket_handle.command(b"SAVE_CONFIG").await?);
            }
            Request::ReloadConfig(response) => {
                debug!("wpa_ctrl config reloaded");
                let _ = response.send(socket_handle.command(b"RECONFIGURE").await?);
            }
            Request::RemoveNetwork(remove_network, response) => {
                let str = match remove_network {
                    RemoveNetwork::All => "all".to_string(),
                    RemoveNetwork::Id(id) => id.to_string(),
                };
                let cmd = format!("REMOVE_NETWORK {str}");
                let bytes = cmd.into_bytes();
                debug!("wpa_ctrl removed network {str}");
                let _ = response.send(socket_handle.command(&bytes).await?);
            }
            Request::SelectNetwork(id, response_sender) => {
                let response_sender = match select_request {
                    None => {
                        let cmd = format!("SELECT_NETWORK {id}");
                        let bytes = cmd.into_bytes();
                        if let Err(e) = socket_handle.command(&bytes).await? {
                            warn!("Error while selecting network {id}: {e}");
                            let _ = response_sender.send(Ok(SelectResult::InvalidNetworkId));
                            None
                        } else {
                            debug!("wpa_ctrl selected network {id}");
                            let status = Self::get_status(socket_handle).await?.unwrap_or_default();
                            if let Some(current_id) = status.get("id") {
                                if current_id == &id.to_string() {
                                    let _ =
                                        response_sender.send(Ok(SelectResult::AlreadyConnected));
                                    None
                                } else {
                                    Some(response_sender)
                                }
                            } else {
                                Some(response_sender)
                            }
                        }
                    }
                    Some(_) => {
                        warn!("Select request already pending! Dropping this one.");
                        let _ = response_sender.send(Ok(SelectResult::PendingSelect));
                        debug!("wpa_ctrl removed network {id}");
                        None
                    }
                };
                if let Some(response_sender) = response_sender {
                    *select_request = Some(SelectRequest::new(
                        self.self_sender.clone(),
                        response_sender,
                        self.select_timeout,
                    ));
                }
            }
            Request::Shutdown => (), //shutdown is handled at the scope above
        }
        Ok(())
    }
}

/// convert to wpa config format idealy a "quoted string"
/// in case of new-lines, quotes or emoji fall back to hex encoding the whole thing
fn conf_escape(raw: &str) -> String {
    if raw.bytes().all(|b| b.is_ascii_graphic() && b != b'"') {
        format!("\"{raw}\"")
    } else {
        hex::encode(raw)
    }
}

struct SelectRequest {
    response: oneshot::Sender<Result<SelectResult>>,
    timeout: tokio::task::JoinHandle<()>,
}

impl SelectRequest {
    fn new(
        sender: mpsc::Sender<Request>,
        response: oneshot::Sender<Result<SelectResult>>,
        timeout: Duration,
    ) -> Self {
        Self {
            response,
            timeout: tokio::task::spawn(async move {
                tokio::time::sleep(timeout).await;
                let _ = sender.send(Request::SelectTimeout).await;
            }),
        }
    }

    fn send(self, result: Result<SelectResult>) {
        self.timeout.abort();
        let _ = self.response.send(result);
    }
}
