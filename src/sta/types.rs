use super::{config, config::unprintf, warn, Result, SocketHandle};
use super::{ParseResult, SocketResult};
use crate::error::ClientError;

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

/// A HashMap of what is returned when running `wpa_cli status`.
pub type Status = HashMap<String, String>;

pub(crate) fn parse_status(response: &str) -> ParseResult<Status> {
    Ok(config::from_str(response)?)
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

/// A WPA pre-shared key, validated at construction.
///
/// wpa_supplicant takes the `psk` field in two distinct forms and encodes them
/// differently on the control socket: a passphrase is sent as a quoted string,
/// while a precomputed 256-bit key is sent as raw, unquoted hex. An unquoted
/// value is always read as a raw key, so a passphrase can never fall back to
/// hex encoding the way an SSID can. The two constructors let the caller say
/// which form it means instead of the library guessing from the string's shape.
///
/// Use [`Psk::passphrase`] or [`Psk::raw`] when the kind is known; parse with
/// `str::parse` to get `wpa_supplicant.conf` semantics (exactly 64 hex digits
/// is a raw key, anything else a passphrase — unambiguous because a passphrase
/// is at most 63 characters).
#[derive(Clone, PartialEq, Eq)]
pub struct Psk(PskInner);

#[derive(Clone, PartialEq, Eq)]
enum PskInner {
    Passphrase(String),
    Raw([u8; 32]),
}

impl Psk {
    /// A WPA passphrase: 8-63 printable-ASCII characters (spaces included).
    ///
    /// A literal double-quote is rejected even though WPA formally allows it:
    /// the control socket has no escape for one inside a quoted value, so it
    /// has no safe representation. Anything else outside printable ASCII has
    /// no valid representation either. Both yield [`ClientError::InvalidPsk`].
    pub fn passphrase(passphrase: impl Into<String>) -> Result<Self> {
        let passphrase = passphrase.into();
        let quotable = passphrase
            .bytes()
            .all(|b| (0x20..=0x7e).contains(&b) && b != b'"');
        if !quotable || !(8..=63).contains(&passphrase.len()) {
            return Err(ClientError::InvalidPsk);
        }
        Ok(Psk(PskInner::Passphrase(passphrase)))
    }

    /// A precomputed 256-bit key, as produced by `wpa_passphrase` or
    /// equivalent PBKDF2 derivation.
    pub fn raw(key: [u8; 32]) -> Self {
        Psk(PskInner::Raw(key))
    }

    /// Encode the value of `SET_NETWORK <id> psk <value>`: a passphrase is
    /// quoted, a raw key is bare lowercase hex.
    pub(crate) fn to_field(&self) -> String {
        match &self.0 {
            PskInner::Passphrase(passphrase) => format!("\"{passphrase}\""),
            PskInner::Raw(key) => hex::encode(key),
        }
    }
}

impl FromStr for Psk {
    type Err = ClientError;

    fn from_str(s: &str) -> Result<Self> {
        if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
            let mut key = [0u8; 32];
            hex::decode_to_slice(s, &mut key).expect("64 hex digits");
            return Ok(Psk::raw(key));
        }
        Self::passphrase(s)
    }
}

/// Never print key material: `Request` (and `SetNetwork` inside it) is logged
/// at debug level with `{:?}`.
impl std::fmt::Debug for Psk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Psk(<redacted>)")
    }
}

/// A BSSID (access-point MAC address).
///
/// wpa_supplicant expects the `bssid` field raw and unquoted (a quoted MAC
/// fails to parse). Parsing up front and re-emitting the canonical
/// `xx:xx:xx:xx:xx:xx` form via [`Display`] means caller input is never echoed
/// into an unquoted command position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bssid([u8; 6]);

impl From<[u8; 6]> for Bssid {
    fn from(mac: [u8; 6]) -> Self {
        Bssid(mac)
    }
}

impl FromStr for Bssid {
    type Err = ClientError;

    fn from_str(s: &str) -> Result<Self> {
        let mut mac = [0u8; 6];
        let mut octets = s.split(':');
        for byte in mac.iter_mut() {
            let octet = octets.next().ok_or(ClientError::InvalidBssid)?;
            // from_str_radix alone would admit signs and whitespace
            if octet.len() != 2 || !octet.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(ClientError::InvalidBssid);
            }
            *byte = u8::from_str_radix(octet, 16).map_err(|_| ClientError::InvalidBssid)?;
        }
        if octets.next().is_some() {
            return Err(ClientError::InvalidBssid);
        }
        Ok(Bssid(mac))
    }
}

impl Display for Bssid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{g:02x}")
    }
}
