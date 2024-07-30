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
    fn from_line(line: &str) -> Option<Self> {
        let (mac, rest) = line.split_once('\t')?;
        let (frequency, rest) = rest.split_once('\t')?;
        let (signal, rest) = rest.split_once('\t')?;
        let signal = isize::from_str(signal).ok()?;
        let (flags, escaped_name) = rest.split_once('\t')?;
        let mut bytes = escaped_name.as_bytes().iter().copied();
        let mut name = vec![];
        // undo "printf_encode"
        loop {
            name.push(match bytes.next() {
                Some(b'\\') => match bytes.next()? {
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'e' => b'\x1b',
                    b'x' => {
                        let hex = [bytes.next()?, bytes.next()?];
                        u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?
                    }
                    c => c,
                },
                Some(c) => c,
                None => break,
            })
        }
        let name = String::from_utf8(name).ok()?;
        Some(ScanResult {
            mac: mac.to_string(),
            frequency: frequency.to_string(),
            signal,
            flags: flags.to_string(),
            name,
        })
    }

    // Overide to allow tabs in the raw string to avoid double escaping everything
    #[allow(clippy::tabs_in_doc_comments)]
    /// Parses lines from a scan result
    ///```
    ///use wifi_ctrl::sta::ScanResult;
    ///let results = ScanResult::vec_from_str(r#"bssid / frequency / signal level / flags / ssid
    ///00:5f:67:90:da:64	2417	-35	[WPA-PSK-CCMP][WPA2-PSK-CCMP][ESS]	TP-Link DA64
    ///e0:91:f5:7d:11:c0	2462	-33	[WPA2-PSK-CCMP][WPS][ESS]	¯\\_(\xe3\x83\x84)_/¯
    ///"#).unwrap();
    ///assert_eq!(results[0].mac, "00:5f:67:90:da:64");
    ///assert_eq!(results[0].name, "TP-Link DA64");
    ///assert_eq!(results[1].signal, -33);
    ///assert_eq!(results[1].name, r#"¯\_(ツ)_/¯"#);
    ///```
    pub fn vec_from_str(response: &str) -> Result<Vec<ScanResult>> {
        let mut results = Vec::new();
        for line in response.lines().skip(1) {
            if let Some(scan_result) = ScanResult::from_line(line) {
                results.push(scan_result);
            } else {
                warn!("Invalid result from scan: {line}");
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
