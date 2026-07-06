use super::*;

use std::time::Duration;

/// Default time to wait for a reply to a control command/request before giving
/// up. Chosen to comfortably cover slower hostapd operations while still
/// unblocking the single-task runtime if a reply never arrives.
const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

/// Default number of `ATTACH`/`LOG_LEVEL` handshake attempts. With
/// [`DEFAULT_ATTACH_RETRY_DELAY`] between tries this bounds the wait to roughly
/// a minute, unlike the socket-open path which retries for 5 minutes.
const DEFAULT_ATTACH_RETRIES: usize = 240;
/// Default delay between attach handshake attempts.
const DEFAULT_ATTACH_RETRY_DELAY: Duration = Duration::from_millis(250);

/// A convenient default type for setting up the WiFiAp process.
pub type WifiSetup = WifiSetupGeneric<32, 32>;

/// The generic WifiSetup struct which has generic constant parameters for adjusting queue size.
/// WiFiSetup type is provided for convenience.
pub struct WifiSetupGeneric<const C: usize = 32, const B: usize = 32> {
    /// Struct for handling runtime process
    wifi: WifiAp,
    /// Client for making requests
    request_client: RequestClient,
}

impl<const C: usize, const B: usize> WifiSetupGeneric<C, B> {
    pub fn new() -> Self {
        // setup the channel for client requests
        let (self_sender, request_receiver) = mpsc::channel(C);
        let request_client = RequestClient::new(self_sender.clone());
        // setup the channel for broadcasts
        let (broadcast_sender, _) = broadcast::channel(B);

        Self {
            wifi: WifiAp {
                socket_path: PATH_DEFAULT_SERVER.into(),
                attach_options: vec![],
                request_receiver,
                broadcast_sender,
                self_sender,
                command_timeout: DEFAULT_COMMAND_TIMEOUT,
                attach_retries: DEFAULT_ATTACH_RETRIES,
                attach_retry_delay: DEFAULT_ATTACH_RETRY_DELAY,
            },
            request_client,
        }
    }

    pub fn set_socket_path<S: Into<std::path::PathBuf>>(&mut self, path: S) {
        self.wifi.socket_path = path.into();
    }

    pub fn add_attach_options(&mut self, options: &[&str]) {
        for o in options {
            self.wifi.attach_options.push(o.to_string());
        }
    }

    /// Set how long to wait for a reply to a control command/request before
    /// giving up with [`ClientError::Timeout`](crate::error::ClientError::Timeout).
    pub fn set_command_timeout(&mut self, timeout: Duration) {
        self.wifi.command_timeout = timeout;
    }

    /// Set how many times to retry the hostapd `ATTACH`/`LOG_LEVEL` handshake
    /// before giving up with
    /// [`SocketError::AttachFailed`](crate::error::SocketError::AttachFailed).
    pub fn set_attach_retries(&mut self, retries: usize) {
        self.wifi.attach_retries = retries;
    }

    /// Set how long to wait between attach handshake attempts.
    pub fn set_attach_retry_delay(&mut self, delay: Duration) {
        self.wifi.attach_retry_delay = delay;
    }

    pub fn get_broadcast_receiver(&self) -> BroadcastReceiver {
        self.wifi.broadcast_sender.subscribe()
    }
    pub fn get_request_client(&self) -> RequestClient {
        self.request_client.clone()
    }

    pub fn complete(self) -> WifiAp {
        self.wifi
    }
}

impl Default for WifiSetupGeneric {
    fn default() -> Self {
        Self::new()
    }
}
