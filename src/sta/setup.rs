use super::*;

/// Default time to wait for a reply to a control command/request before giving
/// up. Chosen to comfortably cover slower wpa_supplicant operations while still
/// unblocking the single-task runtime if a reply never arrives.
const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

/// A convenient default type for setting up the WiFi Station process.
pub type WifiSetup = WifiSetupGeneric<32, 32>;

/// The generic WifiSetup struct which has generic constant parameters for adjusting queue size.
/// WiFiSetup type is provided for convenience.
pub struct WifiSetupGeneric<const C: usize = 32, const B: usize = 32> {
    /// Struct for handling runtime process
    wifi: WifiStation,
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
            wifi: WifiStation {
                socket_path: PATH_DEFAULT_SERVER.into(),
                request_receiver,
                broadcast_sender,
                self_sender,
                select_timeout: Duration::from_secs(10),
                command_timeout: DEFAULT_COMMAND_TIMEOUT,
            },
            request_client,
        }
    }

    pub fn set_socket_path<S: Into<std::path::PathBuf>>(&mut self, path: S) {
        self.wifi.socket_path = path.into();
    }

    pub fn set_select_timeout(&mut self, timeout: Duration) {
        self.wifi.select_timeout = timeout;
    }

    /// Set how long to wait for a reply to a control command/request before
    /// giving up with [`ClientError::Timeout`](crate::error::ClientError::Timeout).
    pub fn set_command_timeout(&mut self, timeout: Duration) {
        self.wifi.command_timeout = timeout;
    }

    pub fn get_broadcast_receiver(&self) -> BroadcastReceiver {
        self.wifi.broadcast_sender.subscribe()
    }
    pub fn get_request_client(&self) -> RequestClient {
        self.request_client.clone()
    }

    pub fn complete(self) -> WifiStation {
        self.wifi
    }
}

impl Default for WifiSetupGeneric {
    fn default() -> Self {
        Self::new()
    }
}
