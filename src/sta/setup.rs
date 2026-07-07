use super::*;

/// Default time to wait for a reply to a control command/request before giving
/// up. Chosen to comfortably cover slower wpa_supplicant operations while still
/// unblocking the single-task runtime if a reply never arrives.
const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

/// Default capacity of the request and broadcast channels.
const DEFAULT_CHANNEL_SIZE: usize = 32;

/// Setup struct for the WiFi Station process.
pub struct WifiSetup {
    /// Struct for handling runtime process
    wifi: WifiStation,
    /// Client for making requests
    request_client: RequestClient,
}

impl WifiSetup {
    pub fn new() -> Self {
        Self::with_capacities(DEFAULT_CHANNEL_SIZE, DEFAULT_CHANNEL_SIZE)
    }

    /// Like [`Self::new`] but with explicit request and broadcast channel
    /// capacities (both default to 32).
    pub fn with_capacities(request_channel_size: usize, broadcast_channel_size: usize) -> Self {
        // setup the channel for client requests
        let (self_sender, request_receiver) = mpsc::channel(request_channel_size);
        let request_client = RequestClient::new(self_sender.clone());
        // setup the sender for broadcasts; receivers subscribe on demand
        let broadcast_sender = broadcast::Sender::new(broadcast_channel_size);

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

impl Default for WifiSetup {
    fn default() -> Self {
        Self::new()
    }
}
