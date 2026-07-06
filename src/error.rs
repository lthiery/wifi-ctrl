use super::*;
use std::convert::Infallible;
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::oneshot::error::RecvError;

/// Error returned by [access point](crate::ap::WifiAp::run) and [station](crate::sta::WifiStation::run) runners if there is
/// a problem with the control socket. e.g. if `wpa_supplicant` is restarted
#[derive(Error, Debug)]
pub enum SocketError {
    /// IO error from control socket
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Client asked runner to shutdown
    #[error("start-up aborted")]
    StartupAborted,
    /// `RequestClient` dropped without shutting down runner
    #[error("internal client channel unexpectedly closed")]
    ClientChannelClosed,
    /// Timeout trying to open the control socket even after retrying
    #[error("timeout opening socket {0}")]
    TimeoutOpeningSocket(String),
    /// Permission denied opening control socket
    #[error("permission denied opening socket {0}")]
    PermissionDeniedOpeningSocket(String),
}

/// Error returned by [access point](crate::ap::RequestClient) and [station](crate::sta::RequestClient) clients if there is
/// a problem with the request e.g. asking to select a network you have not created a config for
#[derive(Error, Debug, Clone)]
pub enum ClientError {
    /// Request failed  e.g. asking to select a network you have not created a config for
    #[error("Supplicant reported request failed")]
    Failed,
    /// Error parsing the response from the socket. This is probably a bug in the [`wifi_ctrl`](crate) code.
    #[error("error {error} parsing response: \n{failed_response}")]
    ParsingResponse {
        #[source]
        error: ParseError,
        failed_response: String,
    },
    /// Timeout waiting for response to request on control socket
    #[error("timeout waiting for response")]
    Timeout,
    /// Request was too big to fit in a datagram, most likely seen on bad custom requests
    #[error("Request was too big only sent {0} of {1} bytes")]
    DidNotWriteAllBytes(usize, usize),
    /// The control socket is not connected at the moment, reconnect and try again
    #[error("Runner task not running")]
    RunnerNotRunning,
    /// A select request is already pending; wait for it to resolve before selecting again
    #[error("Select already pending")]
    PendingSelect,
}

/// A sub error of [`ClientError`] returned when there is a problem parsing the response from
/// the socket. This is probably a bug in the [`wifi_ctrl`](crate) code.
#[derive(Error, Debug, Clone)]
pub enum ParseError {
    #[error("Didn't get expected literal \"OK\" response")]
    NotOK,
    #[error("error parsing config: {0}")]
    ParseConfig(#[from] config::ConfigError),
    #[error("error parsing int: {0}")]
    ParseInt(#[from] std::num::ParseIntError),
    #[error("utf8 error: {0}")]
    Utf8Parse(#[from] std::str::Utf8Error),
}

// Needed to make TryFrom happy when it can't fail
impl From<Infallible> for ParseError {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}

// Happens when the runner half of a request channel gets dropped
// e.g. if it is asked to shut down, or the socket dies
impl<T> From<SendError<T>> for ClientError {
    fn from(_: SendError<T>) -> Self {
        ClientError::RunnerNotRunning
    }
}

// Happens when the runner half of a response channel gets dropped
// e.g. if it is asked to shut down, or the socket dies
impl From<RecvError> for ClientError {
    fn from(_: RecvError) -> Self {
        ClientError::RunnerNotRunning
    }
}
