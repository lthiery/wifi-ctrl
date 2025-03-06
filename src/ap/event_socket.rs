use super::*;

pub(crate) struct EventSocket {
    socket_handle: SocketHandle<1024>,
    attach_options: Vec<String>,
    /// Sends messages to client
    sender: mpsc::Sender<Event>,
}

#[derive(Debug)]
pub(crate) enum Event {
    ApStaConnected(String),
    ApStaDisconnected(String),
    Unknown(String),
}

pub(crate) type EventReceiver = mpsc::Receiver<Event>;

impl EventSocket {
    pub(crate) async fn new<P>(
        socket: P,
        request_receiver: &mut mpsc::Receiver<Request>,
        attach_options: &[String],
    ) -> SocketResult<(EventReceiver, Vec<Request>, Self)>
    where
        P: AsRef<std::path::Path> + std::fmt::Debug,
    {
        let (socket_handle, deferred_requests) =
            SocketHandle::open(socket, "hostapd_async.sock", request_receiver).await?;

        // setup the channel for client requests
        let (sender, receiver) = mpsc::channel(32);
        Ok((
            receiver,
            deferred_requests,
            Self {
                socket_handle,
                sender,
                attach_options: attach_options.to_vec(),
            },
        ))
    }

    async fn send_event(&self, event: Event) -> SocketResult {
        self.sender
            .send(event)
            .await
            .map_err(|_| error::SocketError::EventChannelClosed)?;
        Ok(())
    }

    pub(crate) async fn run(mut self) -> SocketResult {
        let mut command = "ATTACH".to_string();
        for o in &self.attach_options {
            command.push(' ');
            command.push_str(o);
        }
        let mut attach = self.socket_handle.command(command.as_bytes()).await;
        while attach.is_err() {
            tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
            attach = self.socket_handle.command(command.as_bytes()).await;
        }

        let mut log_level = self.socket_handle.command(b"LOG_LEVEL DEBUG").await;
        while log_level.is_err() {
            tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
            log_level = self.socket_handle.command(b"LOG_LEVEL DEBUG").await;
        }
        info!("hostapd event stream registered");

        loop {
            let bytes = self.socket_handle.recv().await?;
            let data_str = String::from_utf8_lossy(bytes);
            let event = if let Some(n) = data_str.find("AP-STA-DISCONNECTED") {
                let index = n + "AP-STA-DISCONNECTED".len();
                let mac = &data_str[index..].trim();
                Event::ApStaDisconnected(mac.to_string())
            } else if let Some(n) = data_str.find("AP-STA-CONNECTED") {
                let index = n + "AP-STA-CONNECTED".len();
                let mac = &data_str[index..].trim();
                Event::ApStaConnected(mac.to_string())
            } else {
                Event::Unknown(data_str.to_string())
            };
            self.send_event(event).await?;
        }
    }
}
