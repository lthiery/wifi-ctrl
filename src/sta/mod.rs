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
    pub async fn run(mut self) -> Result {
        info!("Starting Wifi Station process");
        let (socket_handle, mut deferred_requests) = SocketHandle::open(
            &self.socket_path,
            "mapper_wpa_ctrl_sync.sock",
            &mut self.request_receiver,
        )
        .await?;
        // We start up a separate socket for receiving the "unexpected" events that
        // gets forwarded to us via the unsolicited_receiver
        let (unsolicited_receiver, next_deferred_requests, unsolicited) =
            EventSocket::new(&self.socket_path, &mut self.request_receiver).await?;
        deferred_requests.extend(next_deferred_requests);
        for request in deferred_requests {
            let _ = self.self_sender.send(request).await;
        }
        self.broadcast_sender.send(Broadcast::Ready)?;
        tokio::select!(
            resp = unsolicited.run() => resp,
            resp = self.run_internal(unsolicited_receiver, socket_handle) => resp,
        )
    }

    async fn run_internal(
        mut self,
        mut unsolicited_receiver: EventReceiver,
        mut socket_handle: SocketHandle<10240>,
    ) -> Result {
        // We will collect scan requests and batch respond to them when results are ready
        let mut scan_requests = Vec::new();
        let mut select_request = None;
        loop {
            enum EventOrRequest {
                Event(Option<Event>),
                Request(Option<Request>),
            }

            let event_or_request = tokio::select!(
                unsolicited_msg = unsolicited_receiver.recv() => {
                    EventOrRequest::Event(unsolicited_msg)
                },
                request = self.request_receiver.recv() => {
                    EventOrRequest::Request(request)
                },
            );

            match event_or_request {
                EventOrRequest::Event(event) => match event {
                    Some(unsolicited_msg) => {
                        debug!("Unsolicited event: {unsolicited_msg:?}");
                        Self::handle_event(
                            &mut socket_handle,
                            unsolicited_msg,
                            &mut scan_requests,
                            &mut select_request,
                            &mut self.broadcast_sender,
                        )
                        .await?
                    }
                    None => return Err(error::Error::WifiStationEventChannelClosed),
                },
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
                    None => return Err(error::Error::WifiStationRequestChannelClosed),
                },
            }
        }
    }

    async fn handle_event<const N: usize>(
        socket_handle: &mut SocketHandle<N>,
        event: Event,
        scan_requests: &mut Vec<oneshot::Sender<Result<Arc<Vec<ScanResult>>>>>,
        select_request: &mut Option<SelectRequest>,
        broadcast_sender: &mut broadcast::Sender<Broadcast>,
    ) -> Result {
        match event {
            Event::ScanComplete => {
                let _n = socket_handle.socket.send(b"SCAN_RESULTS").await?;
                let n = socket_handle.socket.recv(&mut socket_handle.buffer).await?;
                let data_str = std::str::from_utf8(&socket_handle.buffer[..n])?;
                let mut scan_results = ScanResult::vec_from_str(data_str);
                scan_results.sort_by(|a, b| a.signal.cmp(&b.signal));

                let results = Arc::new(scan_results);
                while let Some(scan_request) = scan_requests.pop() {
                    if scan_request.send(Ok(results.clone())).is_err() {
                        error!("Scan request response channel closed before response sent");
                    }
                }
            }
            Event::Connected => {
                broadcast_sender.send(Broadcast::Connected)?;
                if let Some(sender) = select_request.take() {
                    sender.send(Ok(SelectResult::Success));
                }
            }
            Event::Disconnected => {
                broadcast_sender.send(Broadcast::Disconnected)?;
            }
            Event::NetworkNotFound => {
                broadcast_sender.send(Broadcast::NetworkNotFound)?;
                if let Some(sender) = select_request.take() {
                    sender.send(Ok(SelectResult::NotFound));
                }
            }
            Event::WrongPsk => {
                broadcast_sender.send(Broadcast::WrongPsk)?;
                if let Some(sender) = select_request.take() {
                    sender.send(Ok(SelectResult::WrongPsk));
                }
            }
            Event::Unknown(msg) => {
                broadcast_sender.send(Broadcast::Unknown(msg))?;
            }
        }
        Ok(())
    }

    async fn get_status<const N: usize>(socket_handle: &mut SocketHandle<N>) -> Result<Status> {
        let _n = socket_handle.socket.send(b"STATUS").await?;
        let n = socket_handle.socket.recv(&mut socket_handle.buffer).await?;
        let data_str = std::str::from_utf8(&socket_handle.buffer[..n])?.trim_end();
        parse_status(data_str)
    }

    async fn handle_request<const N: usize>(
        &self,
        socket_handle: &mut SocketHandle<N>,
        request: Request,
        scan_requests: &mut Vec<oneshot::Sender<Result<Arc<Vec<ScanResult>>>>>,
        select_request: &mut Option<SelectRequest>,
    ) -> Result {
        debug!("Handling request: {request:?}");
        match request {
            Request::Custom(custom, response_channel) => {
                let _n = socket_handle.socket.send(custom.as_bytes()).await?;
                let n = socket_handle.socket.recv(&mut socket_handle.buffer).await?;
                let data_str = std::str::from_utf8(&socket_handle.buffer[..n])?.trim_end();
                debug!("Custom request response: {data_str}");
                if response_channel.send(Ok(data_str.into())).is_err() {
                    error!("Custom request response channel closed before response sent");
                }
            }
            Request::SelectTimeout => {
                if let Some(sender) = select_request.take() {
                    sender.send(Ok(SelectResult::Timeout));
                }
            }
            Request::Scan(response_channel) => {
                scan_requests.push(response_channel);
                if let Err(e) = socket_handle.command(b"SCAN").await {
                    debug!("Error while requesting SCAN: {e}");
                }
            }
            Request::Networks(response_channel) => {
                let _n = socket_handle.socket.send(b"LIST_NETWORKS").await?;
                let n = socket_handle.socket.recv(&mut socket_handle.buffer).await?;
                let data_str = std::str::from_utf8(&socket_handle.buffer[..n])?.trim_end();
                let network_list =
                    NetworkResult::vec_from_str(data_str, &mut socket_handle.socket).await?;
                if response_channel.send(Ok(network_list)).is_err() {
                    error!("Scan request response channel closed before response sent");
                }
            }
            Request::Status(response_channel) => {
                let status = Self::get_status(socket_handle).await;
                if response_channel.send(status).is_err() {
                    error!("Scan request response channel closed before response sent");
                }
            }
            Request::AddNetwork(response_channel) => {
                let _n = socket_handle.socket.send(b"ADD_NETWORK").await?;
                let n = socket_handle.socket.recv(&mut socket_handle.buffer).await?;
                let data_str = std::str::from_utf8(&socket_handle.buffer[..n])?.trim_end();
                let network_id = usize::from_str(data_str)?;
                if response_channel.send(Ok(network_id)).is_err() {
                    error!("Scan request response channel closed before response sent");
                } else {
                    debug!("wpa_ctrl created network {network_id}");
                }
            }
            Request::SetNetwork(id, param, response) => {
                let cmd = format!(
                    "SET_NETWORK {id} {}",
                    match param {
                        SetNetwork::Ssid(ssid) => format!("ssid {}", conf_escape(&ssid)),
                        SetNetwork::Bssid(bssid) => format!("bssid \"{bssid}\""),
                        SetNetwork::Psk(psk) => format!("psk {}", conf_escape(&psk)),
                        SetNetwork::KeyMgmt(mgmt) => format!("key_mgmt {}", mgmt),
                    }
                );
                debug!("wpa_ctrl \"{cmd}\"");
                let bytes = cmd.into_bytes();
                if let Err(e) = socket_handle.command(&bytes).await {
                    warn!("Error while setting network parameter: {e}");
                }
                let _ = response.send(Ok(()));
            }
            Request::SaveConfig(response) => {
                if let Err(e) = socket_handle.command(b"SAVE_CONFIG").await {
                    warn!("Error while saving config: {e}");
                }
                debug!("wpa_ctrl config saved");
                let _ = response.send(Ok(()));
            }
            Request::ReloadConfig(response) => {
                if let Err(e) = socket_handle.command(b"RECONFIGURE").await {
                    warn!("Error while reloading config: {e}");
                }
                debug!("wpa_ctrl config reloaded");
                let _ = response.send(Ok(()));
            }
            Request::RemoveNetwork(remove_network, response) => {
                let str = match remove_network {
                    RemoveNetwork::All => "all".to_string(),
                    RemoveNetwork::Id(id) => id.to_string(),
                };
                let cmd = format!("REMOVE_NETWORK {str}");
                let bytes = cmd.into_bytes();
                if let Err(e) = socket_handle.command(&bytes).await {
                    warn!("Error while removing network {str}: {e}");
                }
                debug!("wpa_ctrl removed network {str}");
                let _ = response.send(Ok(()));
            }
            Request::SelectNetwork(id, response_sender) => {
                let response_sender = match select_request {
                    None => {
                        let cmd = format!("SELECT_NETWORK {id}");
                        let bytes = cmd.into_bytes();
                        if let Err(e) = socket_handle.command(&bytes).await {
                            warn!("Error while selecting network {id}: {e}");
                            let _ = response_sender.send(Ok(SelectResult::InvalidNetworkId));
                            None
                        } else {
                            debug!("wpa_ctrl selected network {id}");
                            let status = Self::get_status(socket_handle).await?;
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
