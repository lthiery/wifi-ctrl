use crate::error::ClientError;

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
            self.self_sender
                .send(request)
                .await
                .expect("self_sender should never close as same struct owns both ends");
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
                    self.handle_event(
                        &mut socket_handle,
                        unsolicited_msg,
                        &mut scan_requests,
                        &mut select_request,
                    )
                    .await?
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

    async fn handle_event<const N: usize>(
        &mut self,
        socket_handle: &mut SocketHandle<N>,
        event: Event,
        scan_requests: &mut Vec<oneshot::Sender<Result<Arc<Vec<ScanResult>>>>>,
        select_request: &mut Option<SelectRequest>,
    ) -> SocketResult {
        match event {
            Event::ScanComplete => {
                let scan_results = socket_handle
                    .request("SCAN_RESULTS", ScanResult::vec_from_str)
                    .await?;
                while let Some(scan_request) = scan_requests.pop() {
                    let _ = scan_request.send(scan_results.clone());
                }
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
        Ok(())
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
                    sender.send(Err(ClientError::Timeout));
                }
            }
            Request::Scan(response_channel) => {
                // wpa_supplicant replies FAIL-BUSY when a scan is already in
                // progress; the pending CTRL-EVENT-SCAN-RESULTS will answer
                // this request too, so treat it as accepted
                match socket_handle
                    .command_matching(b"SCAN", |data| data == "OK" || data == "FAIL-BUSY")
                    .await?
                {
                    Ok(_) => {
                        scan_requests.push(response_channel);
                    }
                    Err(e) => {
                        let _ = response_channel.send(Err(e));
                    }
                };
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
                // Psk and Bssid are validated at construction, so every
                // variant formats infallibly; Psk's Debug impl redacts the
                // key wherever the request is logged.
                let field = match &param {
                    SetNetwork::Ssid(ssid) => format!("ssid {}", conf_escape(ssid)),
                    SetNetwork::Bssid(bssid) => format!("bssid {bssid}"),
                    SetNetwork::Psk(psk) => format!("psk {}", psk.to_field()),
                    SetNetwork::KeyMgmt(mgmt) => format!("key_mgmt {mgmt}"),
                };
                let cmd = format!("SET_NETWORK {id} {field}");
                match &param {
                    SetNetwork::Psk(_) => debug!("wpa_ctrl SET_NETWORK {id} psk <redacted>"),
                    _ => debug!("wpa_ctrl {cmd:?}"),
                }
                let _ = response.send(socket_handle.command(cmd.as_bytes()).await?);
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
                match select_request {
                    None => {
                        let cmd = format!("SELECT_NETWORK {id}");
                        let bytes = cmd.into_bytes();
                        if let Err(e) = socket_handle.command(&bytes).await? {
                            warn!("Error while selecting network {id}: {e}");
                            let _ = response_sender.send(Err(e));
                        } else {
                            debug!("wpa_ctrl selected network {id}");
                            match Self::get_status(socket_handle).await? {
                                Err(e) => {
                                    let _ = response_sender.send(Err(e));
                                }
                                Ok(status) if status.get("id") == Some(&id.to_string()) => {
                                    let _ =
                                        response_sender.send(Ok(SelectResult::AlreadyConnected));
                                }
                                Ok(_) => {
                                    *select_request = Some(SelectRequest::new(
                                        self.self_sender.clone(),
                                        response_sender,
                                        self.select_timeout,
                                    ));
                                }
                            }
                        }
                    }
                    Some(_) => {
                        warn!("Select request already pending! Dropping this one.");
                        let _ = response_sender.send(Err(ClientError::PendingSelect));
                        debug!("wpa_ctrl removed network {id}");
                    }
                };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn psk_passphrase_is_quoted() {
        assert_eq!(
            Psk::passphrase("password123").unwrap().to_field(),
            "\"password123\""
        );
    }

    #[test]
    fn psk_passphrase_with_spaces_is_quoted_not_hex() {
        // A passphrase may contain spaces; it must stay a quoted string, since
        // an unquoted/hex value would be read as a raw pre-shared key.
        assert_eq!(
            Psk::passphrase("correct horse battery").unwrap().to_field(),
            "\"correct horse battery\""
        );
    }

    #[test]
    fn raw_psk_is_bare_hex() {
        let hex_psk = "8dbbe42cb44f21088fbb9cfbf24dc9b39787d6026d436b01b3ac7d34afb4416d";
        let mut key = [0u8; 32];
        hex::decode_to_slice(hex_psk, &mut key).unwrap();
        assert_eq!(Psk::raw(key).to_field(), hex_psk);
    }

    #[test]
    fn psk_from_str_uses_conf_semantics() {
        // Exactly 64 hex digits parses as a raw key, anything else as a
        // passphrase; unambiguous since a passphrase is at most 63 chars.
        let hex_psk = "8dbbe42cb44f21088fbb9cfbf24dc9b39787d6026d436b01b3ac7d34afb4416d";
        assert_eq!(hex_psk.parse::<Psk>().unwrap().to_field(), hex_psk);
        // 63 hex digits is a passphrase
        assert_eq!(
            hex_psk[..63].parse::<Psk>().unwrap().to_field(),
            format!("\"{}\"", &hex_psk[..63])
        );
    }

    #[test]
    fn psk_with_quote_is_rejected() {
        // A literal quote could break out of the quoted value, so it is rejected
        // rather than emitted into the SET_NETWORK command.
        assert!(matches!(
            Psk::passphrase("pass\"; extra"),
            Err(ClientError::InvalidPsk)
        ));
    }

    #[test]
    fn psk_with_control_char_is_rejected() {
        assert!(matches!(
            Psk::passphrase("pass\nword"),
            Err(ClientError::InvalidPsk)
        ));
    }

    #[test]
    fn psk_outside_wpa_length_is_rejected() {
        assert!(matches!(
            Psk::passphrase("short"),
            Err(ClientError::InvalidPsk)
        ));
        assert!(matches!(
            Psk::passphrase("x".repeat(64)),
            Err(ClientError::InvalidPsk)
        ));
    }

    #[test]
    fn psk_debug_is_redacted() {
        let psk = Psk::passphrase("password123").unwrap();
        assert_eq!(format!("{psk:?}"), "Psk(<redacted>)");
        // The whole request is debug-logged by handle_request; the key must
        // not appear through that path either.
        let request = SetNetwork::Psk(psk);
        assert!(!format!("{request:?}").contains("password123"));
    }

    #[test]
    fn bssid_roundtrips_raw_and_unquoted() {
        let bssid: Bssid = "cc:7b:5c:1a:d2:21".parse().unwrap();
        assert_eq!(bssid.to_string(), "cc:7b:5c:1a:d2:21");
        assert_eq!(Bssid::from([0xcc, 0x7b, 0x5c, 0x1a, 0xd2, 0x21]), bssid);
    }

    #[test]
    fn bssid_is_canonicalized() {
        // Mixed case parses but is always emitted lowercase
        let bssid: Bssid = "CC:7B:5C:1A:D2:21".parse().unwrap();
        assert_eq!(bssid.to_string(), "cc:7b:5c:1a:d2:21");
    }

    #[test]
    fn malformed_bssid_is_rejected() {
        for bad in [
            "cc:7b:5c:1a:d2",
            "cc:7b:5c:1a:d2:21:33",
            "cc:7b:5c:1a:d2:21 x",
            "cc:7b:5c:1a:d2:+1",
            "not-a-mac",
            "",
        ] {
            assert!(
                matches!(bad.parse::<Bssid>(), Err(ClientError::InvalidBssid)),
                "expected {bad:?} to be rejected"
            );
        }
    }
}
