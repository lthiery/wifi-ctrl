use super::*;

use std::time::Duration;

/// Maximum number of attempts for the `ATTACH`/`LOG_LEVEL` handshake before
/// giving up. With [`ATTACH_RETRY_DELAY`] between tries this bounds the wait to
/// roughly a minute, unlike the socket-open path which retries for 5 minutes.
const ATTACH_RETRIES: usize = 240;
const ATTACH_RETRY_DELAY: Duration = Duration::from_millis(250);

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
        command_timeout: Duration,
    ) -> SocketResult<(Vec<Request>, Self)>
    where
        P: AsRef<std::path::Path> + std::fmt::Debug,
    {
        let (mut socket_handle, deferred_requests) = SocketHandle::open(
            socket,
            "hostapd_async.sock",
            request_receiver,
            command_timeout,
        )
        .await?;

        let mut command = "ATTACH".to_string();
        for o in attach_options {
            command.push(' ');
            command.push_str(o);
        }
        retry_command(&mut socket_handle, command.as_bytes()).await?;
        retry_command(&mut socket_handle, b"LOG_LEVEL DEBUG").await?;
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

/// Send a control command, retrying on failure with a fixed delay up to
/// [`ATTACH_RETRIES`] times. Returns [`SocketError::AttachFailed`] once the
/// attempts are exhausted so the runtime doesn't spin forever.
async fn retry_command<const N: usize>(
    socket_handle: &mut SocketHandle<N>,
    command: &[u8],
) -> SocketResult {
    for _ in 0..ATTACH_RETRIES {
        if socket_handle.command(command).await?.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(ATTACH_RETRY_DELAY).await;
    }
    Err(error::SocketError::AttachFailed(
        String::from_utf8_lossy(command).to_string(),
    ))
}
