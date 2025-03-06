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
    pub async fn run(&mut self) -> SocketResult {
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
            self.self_sender
                .send(request)
                .await
                .expect("self_sender should never close as same struct owns both ends");
        }
        self.broadcast(Broadcast::Ready);
        tokio::select!(
            resp = event_socket.run() => resp,
            resp = self.run_internal(event_receiver, socket_handle) => resp,
        )
    }

    fn broadcast(&self, event: Broadcast) {
        if self.broadcast_sender.send(event).is_err() {
            debug!("broadcast listener closed")
        }
    }

    async fn run_internal(
        &mut self,
        mut event_receiver: EventReceiver,
        mut socket_handle: SocketHandle<2048>,
    ) -> SocketResult {
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
                    Some(event) => self.handle_event(&mut socket_handle, event).await,
                    None => return Err(error::SocketError::EventChannelClosed),
                },
                EventOrRequest::Request(request) => match request {
                    Some(Request::Shutdown) => return Ok(()),
                    Some(request) => Self::handle_request(&mut socket_handle, request).await?,
                    None => return Err(error::SocketError::ClientChannelClosed),
                },
            }
        }
    }

    async fn handle_event<const N: usize>(
        &self,
        _socket_handle: &mut SocketHandle<N>,
        event_msg: Event,
    ) {
        match event_msg {
            Event::ApStaConnected(mac) => self.broadcast(Broadcast::Connected(mac)),
            Event::ApStaDisconnected(mac) => self.broadcast(Broadcast::Disconnected(mac)),
            Event::Unknown(msg) => self.broadcast(Broadcast::UnknownEvent(msg)),
        };
    }

    async fn handle_request<const N: usize>(
        socket_handle: &mut SocketHandle<N>,
        request: Request,
    ) -> SocketResult {
        debug!("Handling request: {request:?}");
        match request {
            Request::Custom(custom, response_channel) => {
                let data_str = socket_handle.request(&custom, TryInto::try_into).await?;
                debug!("Custom request response: {data_str:?}");
                let _ = response_channel.send(data_str);
            }
            Request::Status(response_channel) => {
                let status = socket_handle
                    .request("STATUS", Status::from_response)
                    .await?;

                let _ = response_channel.send(status);
            }
            Request::Config(response_channel) => {
                let config = socket_handle
                    .request("GET_CONFIG", Config::from_response)
                    .await?;
                let _ = response_channel.send(config);
            }
            Request::Enable(response_channel) => {
                let _ = response_channel.send(socket_handle.command(b"ENABLE").await?);
            }
            Request::Disable(response_channel) => {
                let _ = response_channel.send(socket_handle.command(b"DISABLE").await?);
            }
            Request::SetValue(key, value, response_channel) => {
                let request_string = format!("SET {key} {value}");
                let _ =
                    response_channel.send(socket_handle.command(request_string.as_bytes()).await?);
            }
            Request::Shutdown => (), //shutdown is handled at the scope above
        }
        Ok(())
    }
}
