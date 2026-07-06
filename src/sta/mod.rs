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
                let field = match param {
                    SetNetwork::Ssid(ssid) => Ok(format!("ssid {}", conf_escape(&ssid))),
                    SetNetwork::Bssid(bssid) => bssid_command(&bssid),
                    SetNetwork::Psk(psk) => psk_command(&psk).map(|psk| format!("psk {psk}")),
                    SetNetwork::KeyMgmt(mgmt) => Ok(format!("key_mgmt {mgmt}")),
                };
                match field {
                    Ok(field) => {
                        let cmd = format!("SET_NETWORK {id} {field}");
                        // Never log the PSK; the rest of the SET_NETWORK command is safe.
                        if field.starts_with("psk ") {
                            debug!("wpa_ctrl SET_NETWORK {id} psk <redacted>");
                        } else {
                            debug!("wpa_ctrl {cmd:?}");
                        }
                        let _ = response.send(socket_handle.command(cmd.as_bytes()).await?);
                    }
                    Err(e) => {
                        let _ = response.send(Err(e));
                    }
                }
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

/// Encode a PSK for `SET_NETWORK ... psk`.
///
/// A 64-character hex string is a precomputed PSK, which wpa_supplicant expects
/// raw and unquoted. Anything else is a passphrase and must be quoted: unlike an
/// SSID, a PSK passphrase cannot be hex-encoded, because wpa_supplicant reads an
/// unquoted value as a raw key and would misinterpret the hex.
///
/// A WPA passphrase is 8-63 printable-ASCII characters (spaces included), all
/// safe inside quotes. To keep the quoted form injection-safe we reject anything
/// that could break out of it or that has no valid representation: a literal
/// double-quote, control characters, or non-ASCII bytes all yield
/// [`ClientError::InvalidPsk`] rather than a malformed command.
fn psk_command(psk: &str) -> Result<String> {
    if psk.len() == 64 && psk.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(psk.to_string());
    }
    if psk
        .bytes()
        .any(|b| !(0x20..=0x7e).contains(&b) || b == b'"')
    {
        return Err(ClientError::InvalidPsk);
    }
    Ok(format!("\"{psk}\""))
}

/// Build a `bssid` field for `SET_NETWORK ... bssid`.
///
/// wpa_supplicant expects the BSSID as a raw, unquoted MAC address (quoting it
/// makes the parse fail). Because the value is unquoted, it must be validated as
/// a canonical `xx:xx:xx:xx:xx:xx` MAC so it can't inject extra command tokens;
/// anything else yields [`ClientError::InvalidBssid`].
fn bssid_command(bssid: &str) -> Result<String> {
    let well_formed = bssid.len() == 17
        && bssid.bytes().enumerate().all(|(i, b)| {
            if (i + 1) % 3 == 0 {
                b == b':'
            } else {
                b.is_ascii_hexdigit()
            }
        });
    if well_formed {
        Ok(format!("bssid {bssid}"))
    } else {
        Err(ClientError::InvalidBssid)
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
        assert_eq!(psk_command("password123").unwrap(), "\"password123\"");
    }

    #[test]
    fn psk_passphrase_with_spaces_is_quoted_not_hex() {
        // A passphrase may contain spaces; it must stay a quoted string, since
        // an unquoted/hex value would be read as a raw pre-shared key.
        assert_eq!(
            psk_command("correct horse battery").unwrap(),
            "\"correct horse battery\""
        );
    }

    #[test]
    fn precomputed_psk_is_raw() {
        let hex_psk = "8dbbe42cb44f21088fbb9cfbf24dc9b39787d6026d436b01b3ac7d34afb4416d";
        assert_eq!(hex_psk.len(), 64);
        assert_eq!(psk_command(hex_psk).unwrap(), hex_psk);
    }

    #[test]
    fn psk_with_quote_is_rejected() {
        // A literal quote could break out of the quoted value, so it is rejected
        // rather than emitted into the SET_NETWORK command.
        assert!(matches!(
            psk_command("pass\"; extra"),
            Err(ClientError::InvalidPsk)
        ));
    }

    #[test]
    fn psk_with_control_char_is_rejected() {
        assert!(matches!(
            psk_command("pass\nword"),
            Err(ClientError::InvalidPsk)
        ));
    }

    #[test]
    fn bssid_is_sent_raw_and_unquoted() {
        assert_eq!(
            bssid_command("cc:7b:5c:1a:d2:21").unwrap(),
            "bssid cc:7b:5c:1a:d2:21"
        );
    }

    #[test]
    fn malformed_bssid_is_rejected() {
        for bad in ["cc:7b:5c:1a:d2", "cc:7b:5c:1a:d2:21 x", "not-a-mac", ""] {
            assert!(
                matches!(bssid_command(bad), Err(ClientError::InvalidBssid)),
                "expected {bad:?} to be rejected"
            );
        }
    }
}
