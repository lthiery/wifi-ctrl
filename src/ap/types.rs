use super::{error, Result};
use serde::{Deserialize, Serialize};

/// Status of the WiFi Station
#[derive(Serialize, Deserialize, Debug)]
pub struct Status {
    pub state: String,
    pub phy: String,
    pub freq: u32,
    pub num_sta_non_erp: u64,
    pub num_sta_no_short_slot_time: u64,
    pub num_sta_no_short_preamble: u64,
    pub olbc: u64,
    pub num_sta_ht_no_gf: u64,
    pub num_sta_no_ht: u64,
    pub num_sta_ht_20_mhz: u64,
    pub num_sta_ht40_intolerant: u64,
    pub olbc_ht: u64,
    pub ht_op_mode: String,
    pub cac_time_seconds: u64,
    pub cac_time_left_seconds: Option<u64>,
    pub channel: u64,
    pub secondary_channel: u64,
    pub ieee80211n: u64,
    pub ieee80211ac: u64,
    pub ieee80211ax: u64,
    pub beacon_int: u64,
    pub dtim_period: u64,
    // missing if not not ieee80211n
    pub ht_caps_info: Option<String>,
    pub ht_mcs_bitmask: Option<String>,
    #[serde(default)] // missing if there are no rates
    pub supported_rates: String,
    pub max_txpower: u64,
    pub bss: Vec<String>,
    pub bssid: Vec<String>,
    pub ssid: Vec<String>,
    pub num_sta: Vec<u32>,
}

impl Status {
    /// Decode from the response sent from the hostapd
    /// ```
    /// # use wifi_ctrl::ap::Status;
    /// let resp = r#"
    ///state=ENABLED
    ///phy=phy0
    ///freq=2437
    ///num_sta_non_erp=0
    ///num_sta_no_short_slot_time=0
    ///num_sta_no_short_preamble=0
    ///olbc=0
    ///num_sta_ht_no_gf=0
    ///num_sta_no_ht=0
    ///num_sta_ht_20_mhz=0
    ///num_sta_ht40_intolerant=0
    ///olbc_ht=0
    ///ht_op_mode=0x0
    ///cac_time_seconds=0
    ///cac_time_left_seconds=N/A
    ///channel=6
    ///edmg_enable=0
    ///edmg_channel=0
    ///secondary_channel=0
    ///ieee80211n=0
    ///ieee80211ac=0
    ///ieee80211ax=0
    ///beacon_int=100
    ///dtim_period=2
    ///supported_rates=02 04 0b 16 0c 12 18 24 30 48 60 6c
    ///max_txpower=20
    ///bss[0]=wlan0
    ///bssid[0]=cc:7b:5c:1a:d2:21
    ///ssid[0]=WiFi-SSID
    ///num_sta[0]=0
    ///bss[1]=wlan1
    ///bssid[1]=cc:7b:5c:4d:ff:5c
    ///ssid[1]=¯\\_(\xe3\x83\x84)_/¯
    ///num_sta[1]=1
    ///"#;
    /// let status = Status::from_response(resp).unwrap();
    /// assert_eq!(status.state, "ENABLED");
    /// assert_eq!(status.freq, 2437);
    /// assert_eq!(status.ssid, vec![r"WiFi-SSID", r#"¯\_(ツ)_/¯"#]);
    /// assert_eq!(status.num_sta, vec![0, 1]);
    /// ```
    pub fn from_response(response: &str) -> Result<Status> {
        crate::config::from_str(response).map_err(|e| error::Error::ParsingWifiStatus {
            e,
            s: response.into(),
        })
    }
}

/// Configuration of the WiFi station
#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub bssid: String,
    pub ssid: String,
    pub wps_state: String,
    #[serde(default)] // missing if zero
    pub wpa: i32,
    // missing if WPA is not enabled
    pub key_mgmt: Option<String>,
    pub group_cipher: Option<String>,
    pub rsn_pairwise_cipher: Option<String>,
    pub wpa_pairwise_cipher: Option<String>,
}

impl Config {
    /// Decode from the response sent from the hostapd
    /// ```
    /// # use wifi_ctrl::ap::Config;
    /// let resp = r#"
    ///bssid=cc:7b:5c:1a:d2:21
    ///ssid=WiFi-SSID
    ///wps_state=disabled
    ///wpa=2
    ///key_mgmt=WPA-PSK
    ///group_cipher=CCMP
    ///rsn_pairwise_cipher=CCMP
    ///wpa_pairwise_cipher=CCMP
    ///"#;
    /// let config = Config::from_response(resp).unwrap();
    /// assert_eq!(config.wps_state, "disabled");
    /// assert_eq!(config.wpa, 2);
    /// assert_eq!(config.ssid, "WiFi-SSID");
    /// ```
    pub fn from_response(response: &str) -> Result<Config> {
        crate::config::from_str(response).map_err(|e| error::Error::ParsingWifiConfig {
            e,
            s: response.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_wpa_psk() {
        let resp = r#"
bssid=cc:7b:5c:1a:d2:21
ssid=\xc2\xaf\\_(\xe3\x83\x84)_/\xc2\xaf
wps_state=disabled
wpa=2
key_mgmt=WPA-PSK
group_cipher=CCMP
rsn_pairwise_cipher=CCMP
        "#;
        let config = Config::from_response(resp).unwrap();
        assert_eq!(config.wpa, 2);
        assert_eq!(config.wps_state, "disabled");
        assert_eq!(config.ssid, r#"¯\_(ツ)_/¯"#);
    }

    #[test]
    fn test_config_wsp_1() {
        let resp = r#"
bssid=cc:7b:5c:1a:d2:21
ssid=MY_SSID
wps_state=not configured
passphrase=MY_PASSPHRASE
psk=8dbbe42cb44f21088fbb9cfbf24dc9b39787d6026d436b01b3ac7d34afb4416d
wpa=2
key_mgmt=WPA-PSK
group_cipher=CCMP
rsn_pairwise_cipher=CCMP
        "#;
        let config = Config::from_response(resp).unwrap();
        assert_eq!(config.wpa, 2);
        assert_eq!(config.wps_state, "not configured");
        assert_eq!(config.ssid, "MY_SSID");
    }

    #[test]
    fn test_config_wsp_2() {
        let resp = r#"
bssid=cc:7b:5c:1a:d2:21
ssid=MY_SSID
wps_state=configured
passphrase=MY_PASSPHRASE
psk=8dbbe42cb44f21088fbb9cfbf24dc9b39787d6026d436b01b3ac7d34afb4416d
wpa=2
key_mgmt=WPA-PSK
group_cipher=CCMP
rsn_pairwise_cipher=CCMP
        "#;
        let config = Config::from_response(resp).unwrap();
        assert_eq!(config.wpa, 2);
        assert_eq!(config.wps_state, "configured");
        assert_eq!(config.ssid, "MY_SSID");
    }

    #[test]
    fn test_config_open() {
        let resp = r#"
bssid=cc:7b:5c:1a:d2:21
ssid=Wi-Fi
wps_state=disabled
        "#;
        let config = Config::from_response(resp).unwrap();
        assert_eq!(config.wpa, 0);
        assert_eq!(config.ssid, "Wi-Fi");
    }
}
