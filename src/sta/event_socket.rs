use super::*;

pub(crate) struct EventSocket {
    socket_handle: SocketHandle<1024>,
}

#[derive(Debug)]
pub(crate) enum Event {
    ScanComplete,
    ScanFailed,
    Connected,
    Disconnected,
    NetworkNotFound,
    WrongPsk,
    Unknown(String),
}

impl EventSocket {
    pub(crate) async fn new<P>(
        socket: P,
        request_receiver: &mut mpsc::Receiver<Request>,
    ) -> SocketResult<(Vec<Request>, Self)>
    where
        P: AsRef<std::path::Path> + std::fmt::Debug,
    {
        let (socket_handle, deferred_requests) =
            SocketHandle::open(socket, "wpa_ctrl_async.sock", request_receiver).await?;
        info!("wpa_ctrl attempting attach");
        socket_handle.socket.send(b"ATTACH").await?;
        Ok((deferred_requests, Self { socket_handle }))
    }

    pub(crate) async fn recv(&mut self) -> SocketResult<Event> {
        let bytes = self.socket_handle.recv().await?;
        let data_str = String::from_utf8_lossy(bytes);
        debug!("wpa_ctrl event: {data_str}");
        Ok(
            if data_str.trim_end().ends_with("CTRL-EVENT-SCAN-RESULTS") {
                Event::ScanComplete
            } else if data_str.contains("CTRL-EVENT-SCAN-FAILED") {
                Event::ScanFailed
            } else if data_str.contains("CTRL-EVENT-CONNECTED") {
                Event::Connected
            } else if data_str.contains("CTRL-EVENT-DISCONNECTED") {
                Event::Disconnected
            } else if data_str.contains("CTRL-EVENT-NETWORK-NOT-FOUND") {
                Event::NetworkNotFound
            } else if data_str.contains("CTRL-EVENT-SSID-TEMP-DISABLED")
                && data_str.contains("reason=WRONG_KEY")
            {
                Event::WrongPsk
            } else {
                Event::Unknown(data_str.trim_end().into())
            },
        )
    }
}
