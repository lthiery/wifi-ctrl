use super::{config, config::unprintf, warn, Result, SocketHandle};
use super::{ParseResult, SocketResult};

use serde::Serialize;
use std::collections::HashMap;
use std::fmt::Display;
use std::str::FromStr;
use std::sync::Arc;

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
        let name = unprintf(escaped_name).ok()?;
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
    pub fn vec_from_str(response: &str) -> ParseResult<Arc<Vec<ScanResult>>> {
        let mut results = Vec::new();
        for line in response.lines().skip(1) {
            // skip lines we can't parse so one odd entry doesn't fail the
            // whole scan
            if let Some(scan_result) = ScanResult::from_line(line) {
                results.push(scan_result);
            } else {
                warn!("Invalid result from scan: {line}");
            }
        }
        results.sort_by_key(|a| a.signal);
        Ok(Arc::new(results))
    }
}

#[derive(Serialize, Debug, Clone)]
/// A known WiFi network.
pub struct NetworkResult {
    pub network_id: usize,
    pub ssid: String,
    pub flags: String,
}

fn parse_get_network(resp: &str) -> ParseResult<String> {
    let escaped = resp.trim_matches('\"');
    Ok(unprintf(escaped)?)
}

impl NetworkResult {
    pub async fn request_results<const N: usize>(
        socket_handle: &mut SocketHandle<N>,
    ) -> SocketResult<Result<Vec<NetworkResult>>> {
        let response: String = match socket_handle
            .request("LIST_NETWORKS", TryInto::try_into)
            .await?
        {
            Ok(x) => x,
            Err(e) => return Ok(Err(e)),
        };
        let mut results = Vec::new();
        let split = response.split('\n').skip(1);
        for line in split {
            let mut line_split = line.split_whitespace();
            if let Some(network_id) = line_split.next() {
                if let Ok(network_id) = usize::from_str(network_id) {
                    let ssid = match socket_handle
                        .request(&format!("GET_NETWORK {network_id} ssid"), parse_get_network)
                        .await?
                    {
                        Ok(x) => x,
                        Err(e) => return Ok(Err(e)),
                    };
                    if let Some(flags) = line_split.last() {
                        results.push(NetworkResult {
                            flags: flags.into(),
                            ssid,
                            network_id,
                        })
                    }
                } else {
                    warn!("Invalid network_id: {network_id}")
                }
            }
        }
        Ok(Ok(results))
    }
}

/// Parsed output of `wpa_cli status`.
///
/// The commonly-present fields are typed for convenience; everything the
/// supplicant reports is also available untouched via [`Status::raw`] /
/// [`Status::get`], so newer or driver-specific keys are never lost. Missing
/// keys simply leave the corresponding field as `None` rather than failing the
/// whole parse.
#[derive(Serialize, Debug, Clone, Default)]
pub struct Status {
    pub wpa_state: Option<String>,
    pub ssid: Option<String>,
    pub bssid: Option<String>,
    pub id: Option<usize>,
    pub freq: Option<u32>,
    pub address: Option<String>,
    pub ip_address: Option<String>,
    pub key_mgmt: Option<String>,
    pub mode: Option<String>,
    /// Every key/value pair from the response, including those surfaced as
    /// typed fields above.
    pub raw: HashMap<String, String>,
}

impl Status {
    /// Look up a raw status field the typed accessors don't cover.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.raw.get(key).map(String::as_str)
    }
}

pub(crate) fn parse_status(response: &str) -> ParseResult<Status> {
    let raw: HashMap<String, String> = config::from_str(response)?;
    Ok(Status {
        wpa_state: raw.get("wpa_state").cloned(),
        ssid: raw.get("ssid").cloned(),
        bssid: raw.get("bssid").cloned(),
        id: raw.get("id").and_then(|v| v.parse().ok()),
        freq: raw.get("freq").and_then(|v| v.parse().ok()),
        address: raw.get("address").cloned(),
        ip_address: raw.get("ip_address").cloned(),
        key_mgmt: raw.get("key_mgmt").cloned(),
        mode: raw.get("mode").cloned(),
        raw,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_types_known_fields_and_keeps_raw() {
        let resp = "\
bssid=cc:7b:5c:1a:d2:21
freq=2412
ssid=my-network
id=3
mode=station
wpa_state=COMPLETED
address=aa:bb:cc:dd:ee:ff
ip_address=192.168.1.42
some_future_key=42";
        let status = parse_status(resp).unwrap();
        assert_eq!(status.wpa_state.as_deref(), Some("COMPLETED"));
        assert_eq!(status.ssid.as_deref(), Some("my-network"));
        assert_eq!(status.id, Some(3));
        assert_eq!(status.freq, Some(2412));
        assert_eq!(status.ip_address.as_deref(), Some("192.168.1.42"));
        // key_mgmt absent from the response -> None, not a parse failure
        assert_eq!(status.key_mgmt, None);
        // unknown keys are preserved via the raw escape hatch
        assert_eq!(status.get("some_future_key"), Some("42"));
    }

    #[test]
    fn parse_status_tolerates_sparse_response() {
        let status = parse_status("wpa_state=SCANNING").unwrap();
        assert_eq!(status.wpa_state.as_deref(), Some("SCANNING"));
        assert_eq!(status.ssid, None);
        assert_eq!(status.id, None);
    }
}
