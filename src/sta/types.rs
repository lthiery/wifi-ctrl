use super::{error, warn, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::Display;
use std::str::FromStr;
use tokio::net::UnixDatagram;

#[derive(Serialize, Debug, Clone)]
/// The result from scanning for networks.
pub struct ScanResult {
    pub mac: String,
    pub frequency: String,
    pub signal: isize,
    pub flags: String,
    pub name: String,
}

impl ScanResult {
    pub fn vec_from_str(response: &str) -> Result<Vec<ScanResult>> {
        let mut results = Vec::new();
        let split = response.split('\n').skip(1);
        for line in split {
            let mut line_split = line.split_whitespace();
            if let (Some(mac), Some(frequency), Some(signal), Some(flags)) = (
                line_split.next(),
                line_split.next(),
                line_split.next(),
                line_split.next(),
            ) {
                let mut name: Option<String> = None;
                for text in line_split {
                    match &mut name {
                        Some(started) => {
                            started.push(' ');
                            started.push_str(text);
                        }
                        None => {
                            name = Some(text.to_string());
                        }
                    }
                }
                if let Some(name) = name {
                    if let Ok(signal) = isize::from_str(signal) {
                        let scan_result = ScanResult {
                            mac: mac.to_string(),
                            frequency: frequency.to_string(),
                            signal,
                            flags: flags.to_string(),
                            name,
                        };
                        results.push(scan_result);
                    } else {
                        warn!("Invalid string for signal: {signal}");
                    }
                }
            }
        }
        Ok(results)
    }
}

#[derive(Serialize, Debug, Clone)]
/// A known WiFi network.
pub struct NetworkResult {
    pub network_id: usize,
    pub ssid: String,
    pub flags: String,
}

impl NetworkResult {
    pub async fn vec_from_str(
        response: &str,
        socket: &mut UnixDatagram,
    ) -> Result<Vec<NetworkResult>> {
        let mut buffer = [0; 256];
        let mut results = Vec::new();
        let split = response.split('\n').skip(1);
        for line in split {
            let mut line_split = line.split_whitespace();
            if let Some(network_id) = line_split.next() {
                let cmd = format!("GET_NETWORK {network_id} ssid");
                let bytes = cmd.into_bytes();
                socket.send(&bytes).await?;
                let n = socket.recv(&mut buffer).await?;
                let ssid = std::str::from_utf8(&buffer[..n])?.trim_matches('\"');
                if let Ok(network_id) = usize::from_str(network_id) {
                    if let Some(flags) = line_split.last() {
                        results.push(NetworkResult {
                            flags: flags.into(),
                            ssid: ssid.into(),
                            network_id,
                        })
                    }
                } else {
                    warn!("Invalid network_id: {network_id}")
                }
            }
        }
        Ok(results)
    }
}

/// A HashMap of what is returned when running `wpa_cli status`.
pub type Status = HashMap<String, String>;

pub(crate) fn parse_status(response: &str) -> Result<Status> {
    use config::{Config, File, FileFormat};
    let config = Config::builder()
        .add_source(File::from_str(response, FileFormat::Ini))
        .build()
        .map_err(|e| error::Error::ParsingWifiStatus {
            e,
            s: response.into(),
        })?;
    Ok(config.try_deserialize::<HashMap<String, String>>().unwrap())
}

#[derive(Debug)]
/// Key management types for WiFi networks (eg: WPA-PSK, WPA-EAP, etc). In theory, more than one may
/// be configured, but I believe `wpa_supplicant` defaults to all of them if omitted. Therefore, in
/// practice, this is mostly important for setting `key_mgmt` to `None` for an open network.
pub enum KeyMgmt {
    None,
    WpaPsk,
    WpaEap,
    IEEE8021X,
}

impl Display for KeyMgmt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            KeyMgmt::None => "NONE".to_string(),
            KeyMgmt::WpaPsk => "WPA-PSK".to_string(),
            KeyMgmt::WpaEap => "WPA-EAP".to_string(),
            KeyMgmt::IEEE8021X => "IEEE8021X".to_string(),
        };
        write!(f, "{}", str)
    }
}
