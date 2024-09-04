use super::*;
use std::io::ErrorKind;
use tokio::net::UnixDatagram;

pub struct SocketHandle<const N: usize> {
    #[allow(unused)]
    /// Temporary directory for socket. If it drops, socket breaks.
    tmp_dir: tempfile::TempDir,
    /// Socket for synchronous messages
    pub socket: UnixDatagram,
    pub buffer: [u8; N],
}

const RETRY_MINUTES: u64 = 5;

impl<const N: usize> SocketHandle<N> {
    pub(crate) async fn open<P, S>(
        path: P,
        label: &str,
        request_channel: &mut mpsc::Receiver<S>,
    ) -> Result<(Self, Vec<S>)>
    where
        P: AsRef<std::path::Path> + std::fmt::Debug,
        S: ShutdownSignal,
    {
        let tmp_dir = tempfile::tempdir()?;
        let connect_from = tmp_dir.path().join(label);
        let socket = UnixDatagram::bind(connect_from)?;
        let socket_debug = &format!("{path:?}");
        // loop around waiting for the socket for up to 5 minutes
        let mut deferred_requests = Vec::new();
        let deferred_requests_handle = &mut deferred_requests;
        let socket = tokio::select!(
            resp = async move  {
                let mut loop_count = 0;
                let s: Result<UnixDatagram> = loop {
                    match socket.connect(&path) {
                        Ok(()) => break Ok(socket),
                        Err(e) => {
                            // if socket is there but permission denied, fail fast
                            if e.kind() == ErrorKind::PermissionDenied {
                                break Err(error::Error::PermissionDeniedOpeningSocket(socket_debug.to_string()));
                            }
                            if loop_count % 60 == 0 {
                                info!("Failed to connect to {socket_debug}, retrying for {} more minutes", RETRY_MINUTES-(loop_count+1)/60);
                            }
                            loop_count+=1;
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        }
                    }
                };
                s
            } => resp,
            _ = async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(60*RETRY_MINUTES)).await;
            } => Err(error::Error::TimeoutOpeningSocket(socket_debug.to_string())),
            _ = async move {
                loop {
                    if let Some(request) = request_channel.recv().await {
                        if request.is_shutdown() {
                            break;
                        } else {
                            deferred_requests_handle.push(request);
                        }
                    }
                }
            } => Err(error::Error::StartupAborted),
        );

        if let Err(error::Error::StartupAborted) = socket {
            for request in deferred_requests {
                request.inform_of_shutdown();
            }
            return Err(error::Error::StartupAborted);
        }

        Ok((
            Self {
                tmp_dir,
                socket: socket?,
                buffer: [0; N],
            },
            deferred_requests,
        ))
    }

    pub async fn command(&mut self, cmd: &[u8]) -> Result {
        let n = self.socket.send(cmd).await?;
        if n != cmd.len() {
            return Err(error::Error::DidNotWriteAllBytes(n, cmd.len()));
        }
        self.expect_ok_with_default_timeout().await
    }

    pub async fn request(&mut self, req: &str) -> Result<&str> {
        let n = self.socket.send(req.as_bytes()).await?;
        if n != req.len() {
            return Err(error::Error::DidNotWriteAllBytes(n, req.len()));
        }
        self.recv_line().await
    }

    pub async fn recv_line(&mut self) -> Result<&str> {
        let n = self.socket.recv(&mut self.buffer).await?;
        Ok(std::str::from_utf8(&self.buffer[..n])?.trim_end())
    }

    async fn expect_ok(&mut self) -> Result {
        match self.socket.recv(&mut self.buffer).await {
            Ok(n) => {
                let data_str = std::str::from_utf8(&self.buffer[..n])?.trim_end();
                if data_str.trim() == "OK" {
                    Ok(())
                } else {
                    Err(error::Error::UnexpectedWifiApRepsonse(data_str.into()))
                }
            }
            Err(e) => Err(error::Error::UnsolicitedIoError(e)),
        }
    }

    async fn expect_ok_with_default_timeout(&mut self) -> Result {
        self.expect_ok_with_timeout(tokio::time::Duration::from_secs(1))
            .await
    }

    pub async fn expect_ok_with_timeout(&mut self, timeout: tokio::time::Duration) -> Result {
        tokio::select!(
            resp = self.expect_ok() => resp,
            _ =
                tokio::time::sleep(timeout) => Err(error::Error::Timeout)
        )
    }
}
