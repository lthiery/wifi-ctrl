use super::*;

/// A convenient default type for setting up the WiFi Station process.
pub type WifiSetup = WifiSetupGeneric<32, 32>;

/// The generic WifiSetup struct which has generic constant parameters for adjusting queue size.
/// WiFiSetup type is provided for convenience.
pub struct WifiSetupGeneric<const C: usize = 32, const B: usize = 32> {
    /// Struct for handling runtime process
    wifi: WifiStation,
    /// Client for making requests
    request_client: RequestClient,
    #[allow(unused)]
    /// Client for receiving alerts
    broadcast_receiver: BroadcastReceiver,
}

impl<const C: usize, const B: usize> WifiSetupGeneric<C, B> {
    pub fn new() -> Result<Self> {
        // setup the channel for client requests
        let (self_sender, request_receiver) = mpsc::channel(C);
        let request_client = RequestClient::new(self_sender.clone());
        // setup the channel for broadcasts
        let (broadcast_sender, broadcast_receiver) = broadcast::channel(B);

        Ok(Self {
            wifi: WifiStation {
                socket_path: PATH_DEFAULT_SERVER.into(),
                request_receiver,
                broadcast_sender,
                self_sender,
                select_timeout: Duration::from_secs(10),
            },
            request_client,
            broadcast_receiver,
        })
    }

    pub fn set_socket_path<S: Into<std::path::PathBuf>>(&mut self, path: S) {
        self.wifi.socket_path = path.into();
    }

    pub fn set_select_timeout(&mut self, timeout: Duration) {
        self.wifi.select_timeout = timeout;
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
