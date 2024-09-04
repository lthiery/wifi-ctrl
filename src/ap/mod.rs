use super::*;

mod types;
pub use types::*;

mod client;
pub use client::*;

mod setup;
pub use setup::*;

mod event_socket;
use event_socket::*;

const PATH_DEFAULT_SERVER: &str = "/var/run/hostapd/wlan1";

/// Instance that runs the Wifi process
pub struct WifiAp {
    /// Path to the socket
    socket_path: std::path::PathBuf,
    /// Options to pass to the hostapd attach command
    attach_options: Vec<String>,
    /// Channel for receiving requests
    request_receiver: mpsc::Receiver<Request>,
    #[allow(unused)]
    /// Channel for broadcasting alerts
    broadcast_sender: broadcast::Sender<Broadcast>,
    /// Channel for sending requests to itself
    self_sender: mpsc::Sender<Request>,
}

impl WifiAp {
    pub async fn run(mut self) -> Result {
        info!("Starting Wifi AP process");
        let (event_receiver, mut deferred_requests, event_socket) = EventSocket::new(
            &self.socket_path,
            &mut self.request_receiver,
            &self.attach_options,
        )
        .await?;
        // We start up a separate socket for receiving the "unexpected" events that
        // gets forwarded to us via the event_receiver
        let (socket_handle, next_deferred_requests) = SocketHandle::open(
            &self.socket_path,
            "mapper_hostapd_sync.sock",
            &mut self.request_receiver,
        )
        .await?;
        deferred_requests.extend(next_deferred_requests);
        for request in deferred_requests {
            let _ = self.self_sender.send(request).await;
        }
        self.broadcast_sender.send(Broadcast::Ready)?;
        tokio::select!(
            resp = event_socket.run() => resp,
            resp = self.run_internal(event_receiver, socket_handle) => resp,
        )
    }

    async fn run_internal(
        mut self,
        mut event_receiver: EventReceiver,
        mut socket_handle: SocketHandle<2048>,
    ) -> Result {
        enum EventOrRequest {
            Event(Option<Event>),
            Request(Option<Request>),
        }

        loop {
            let event_or_request = tokio::select!(
                event = event_receiver.recv() => EventOrRequest::Event(event),
                request = self.request_receiver.recv() => EventOrRequest::Request(request),
            );
            match event_or_request {
                EventOrRequest::Event(event) => match event {
                    Some(event) => {
                        Self::handle_event(&mut socket_handle, &self.broadcast_sender, event)
                            .await?
                    }
                    None => return Err(error::Error::WifiApEventChannelClosed),
                },
                EventOrRequest::Request(request) => match request {
                    Some(Request::Shutdown) => return Ok(()),
                    Some(request) => Self::handle_request(&mut socket_handle, request).await?,
                    None => return Err(error::Error::WifiApRequestChannelClosed),
                },
            }
        }
    }

    async fn handle_event<const N: usize>(
        _socket_handle: &mut SocketHandle<N>,
        broadcast_sender: &broadcast::Sender<Broadcast>,
        event_msg: Event,
    ) -> Result {
        match event_msg {
            Event::ApStaConnected(mac) => {
                if let Err(e) = broadcast_sender.send(Broadcast::Connected(mac)) {
                    warn!("error broadcasting: {e}");
                }
            }
            Event::ApStaDisconnected(mac) => {
                if let Err(e) = broadcast_sender.send(Broadcast::Disconnected(mac)) {
                    warn!("error broadcasting: {e}");
                }
            }
            Event::Unknown(msg) => {
                if let Err(e) = broadcast_sender.send(Broadcast::UnknownEvent(msg)) {
                    warn!("error broadcasting: {e}");
                }
            }
        };
        Ok(())
    }

    async fn handle_request<const N: usize>(
        socket_handle: &mut SocketHandle<N>,
        request: Request,
    ) -> Result {
        debug!("Handling request: {request:?}");
        match request {
            Request::Custom(custom, response_channel) => {
                let _n = socket_handle.socket.send(custom.as_bytes()).await?;
                let data_str = socket_handle.recv_line().await?;
                debug!("Custom request response: {data_str}");
                if response_channel.send(Ok(data_str.into())).is_err() {
                    error!("Custom request response channel closed before response sent");
                }
            }
            Request::Status(response_channel) => {
                let _n = socket_handle.socket.send(b"STATUS").await?;
                let data_str = socket_handle.recv_line().await?;
                let status = Status::from_response(data_str)?;

                if response_channel.send(Ok(status)).is_err() {
                    error!("Status request response channel closed before response sent");
                }
            }
            Request::Config(response_channel) => {
                let _n = socket_handle.socket.send(b"GET_CONFIG").await?;
                let data_str = socket_handle.recv_line().await?;
                let config = Config::from_response(data_str)?;

                if response_channel.send(Ok(config)).is_err() {
                    error!("Config request response channel closed before response sent");
                }
            }
            Request::Enable(response_channel) => {
                Self::ok_fail_request(socket_handle, b"ENABLE", response_channel).await?
            }
            Request::Disable(response_channel) => {
                Self::ok_fail_request(socket_handle, b"DISABLE", response_channel).await?
            }
            Request::SetValue(key, value, response_channel) => {
                let request_string = format!("SET {key} {value}");
                Self::ok_fail_request(socket_handle, request_string.as_bytes(), response_channel)
                    .await?
            }
            Request::Shutdown => (), //shutdown is handled at the scope above
        }
        Ok(())
    }

    async fn ok_fail_request<const N: usize>(
        socket_handle: &mut SocketHandle<N>,
        request: &[u8],
        response_channel: oneshot::Sender<Result>,
    ) -> Result {
        let _n = socket_handle.socket.send(request).await?;
        let data_str = socket_handle.recv_line().await?;
        let response = if data_str == "OK" {
            Ok(())
        } else {
            Err(error::Error::UnexpectedWifiApRepsonse(data_str.into()))
        };

        if response_channel.send(response).is_err() {
            error!("Config request response channel closed before response sent");
        }
        Ok(())
    }
}
