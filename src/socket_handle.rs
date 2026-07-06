use super::*;
use error::{ClientError, ParseError};
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
    ) -> SocketResult<(Self, Vec<S>)>
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
                let s: SocketResult<UnixDatagram> = loop {
                    match socket.connect(&path) {
                        Ok(()) => break Ok(socket),
                        Err(e) => {
                            // if socket is there but permission denied, fail fast
                            if e.kind() == ErrorKind::PermissionDenied {
                                break Err(error::SocketError::PermissionDeniedOpeningSocket(socket_debug.to_string()));
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
            } => Err(error::SocketError::TimeoutOpeningSocket(socket_debug.to_string())),
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
            } => Err(error::SocketError::StartupAborted),
        );

        Ok((
            Self {
                tmp_dir,
                socket: socket?,
                buffer: [0; N],
            },
            deferred_requests,
        ))
    }

    pub async fn recv(&mut self) -> SocketResult<&[u8]> {
        let n = self.socket.recv(&mut self.buffer).await?;
        Ok(&self.buffer[..n])
    }

    pub async fn command(&mut self, cmd: &[u8]) -> SocketResult<Result> {
        self.command_matching(cmd, |data| data == "OK").await
    }

    /// Like [`Self::command`] but with a custom set of accepted responses,
    /// for commands whose success reply isn't just "OK".
    pub async fn command_matching(
        &mut self,
        cmd: &[u8],
        accept: impl Fn(&str) -> bool,
    ) -> SocketResult<Result> {
        let n = self.socket.send(cmd).await?;
        if n != cmd.len() {
            return Ok(Err(error::ClientError::DidNotWriteAllBytes(n, cmd.len())));
        }
        let parse = |data: &str| {
            if accept(data) {
                Ok(())
            } else {
                Err(error::ParseError::NotOK)
            }
        };
        tokio::select!(
            resp = self.parse_resp(parse) => resp,
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) =>
                Ok(Err(error::ClientError::Timeout)),
        )
    }

    pub(crate) async fn request<'a, T, E, F>(
        &'a mut self,
        req: &str,
        parse: F,
    ) -> SocketResult<Result<T>>
    where
        ParseError: From<E>,
        F: FnOnce(&'a str) -> std::result::Result<T, E>,
    {
        let n = self.socket.send(req.as_bytes()).await?;
        if n != req.len() {
            return Ok(Err(error::ClientError::DidNotWriteAllBytes(n, req.len())));
        }
        self.parse_resp(parse).await
    }

    async fn parse_resp<'a, T, E, F>(&'a mut self, parse: F) -> SocketResult<Result<T>>
    where
        ParseError: From<E>,
        F: FnOnce(&'a str) -> std::result::Result<T, E>,
    {
        let bytes = self.recv().await?;
        let str = std::str::from_utf8(bytes).map(|r| r.trim_end_matches('\n'));
        Ok(str
            .map_err(Into::<ParseError>::into)
            .and_then(|s| parse(s).map_err(Into::<ParseError>::into))
            .map_err(|error| {
                if str == Ok("FAIL") {
                    ClientError::Failed
                } else {
                    ClientError::ParsingResponse {
                        error,
                        failed_response: String::from_utf8_lossy(bytes).to_string(),
                    }
                }
            }))
    }
}
