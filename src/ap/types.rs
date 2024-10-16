use super::{error, Result};
use serde::{Deserialize, Serialize};

/// Status of the WiFi Station
#[derive(Serialize, Deserialize, Debug)]
pub struct Status {
    pub state: String,
    pub phy: String,
    pub freq: String,
    pub num_sta_non_erp: String,
    pub num_sta_no_short_slot_time: String,
    pub num_sta_no_short_preamble: String,
    pub olbc: String,
    pub num_sta_ht_no_gf: String,
    pub num_sta_no_ht: String,
    pub num_sta_ht_20_mhz: String,
    pub num_sta_ht40_intolerant: String,
    pub olbc_ht: String,
    pub ht_op_mode: String,
    pub cac_time_seconds: String,
    pub cac_time_left_seconds: String,
    pub channel: String,
    pub secondary_channel: String,
    pub ieee80211n: String,
    pub ieee80211ac: String,
    pub ieee80211ax: String,
    pub beacon_int: String,
    pub dtim_period: String,
    pub ht_caps_info: String,
    pub ht_mcs_bitmask: String,
    pub supported_rates: String,
    pub max_txpower: String,
    pub bss: Vec<String>,
    pub bssid: Vec<String>,
    pub ssid: Vec<String>,
    pub num_sta: Vec<String>,
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
    ///ht_caps_info=foo,
    ///ht_mcs_bitmask=bar,
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
    /// assert_eq!(status.freq, "2437");
    /// assert_eq!(status.ssid, vec![r"WiFi-SSID", r#"¯\_(ツ)_/¯"#]);
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
    pub wps_state: bool,
    pub wpa: i32,
    pub key_mgmt: String,
    pub group_cipher: String,
    pub rsn_pairwise_cipher: String,
    pub wpa_pairwise_cipher: String,
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
    /// assert_eq!(config.wps_state, false);
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
