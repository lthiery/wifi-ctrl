use super::*;

pub(crate) struct EventSocket {
    socket_handle: SocketHandle<1024>,
}

#[derive(Debug)]
pub(crate) enum Event {
    ApStaConnected(String),
    ApStaDisconnected(String),
    Unknown(String),
}

impl EventSocket {
    pub(crate) async fn new<P>(
        socket: P,
        request_receiver: &mut mpsc::Receiver<Request>,
        attach_options: &[String],
    ) -> SocketResult<(Vec<Request>, Self)>
    where
        P: AsRef<std::path::Path> + std::fmt::Debug,
    {
        let (mut socket_handle, deferred_requests) =
            SocketHandle::open(socket, "hostapd_async.sock", request_receiver).await?;

        let mut command = "ATTACH".to_string();
        for o in attach_options {
            command.push(' ');
            command.push_str(o);
        }
        let mut attach = socket_handle.command(command.as_bytes()).await?;
        while attach.is_err() {
            tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
            attach = socket_handle.command(command.as_bytes()).await?;
        }

        let mut log_level = socket_handle.command(b"LOG_LEVEL DEBUG").await?;
        while log_level.is_err() {
            tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
            log_level = socket_handle.command(b"LOG_LEVEL DEBUG").await?;
        }
        info!("hostapd event stream registered");
        Ok((deferred_requests, Self { socket_handle }))
    }

    pub(crate) async fn recv(&mut self) -> SocketResult<Event> {
        let bytes = self.socket_handle.recv().await?;
        let data_str = String::from_utf8_lossy(bytes);
        Ok(if let Some(n) = data_str.find("AP-STA-DISCONNECTED") {
            let index = n + "AP-STA-DISCONNECTED".len();
            let mac = data_str[index..].trim();
            Event::ApStaDisconnected(mac.to_string())
        } else if let Some(n) = data_str.find("AP-STA-CONNECTED") {
            let index = n + "AP-STA-CONNECTED".len();
            let mac = &data_str[index..].trim();
            Event::ApStaConnected(mac.to_string())
        } else {
            Event::Unknown(data_str.to_string())
        })
    }
}
