//! Tokio-based runtimes for communicating with hostapd and wpa-supplicant.
//!
//! Use [`sta`] to run a WiFi station (network client) against `wpa_supplicant`,
//! and [`ap`] to run an access point against `hostapd`.
//!
//! # Quick Start
//!
//! ```no_run
//! use wifi_ctrl::sta;
//!
//! #[tokio::main]
//! async fn main() -> wifi_ctrl::Result {
//!     let mut setup = sta::WifiSetup::new();
//!     setup.set_socket_path("/var/run/wpa_supplicant/wlan0");
//!     let requester = setup.get_request_client();
//!     let mut runtime = setup.complete();
//!     tokio::spawn(async move { runtime.run().await });
//!
//!     let scan = requester.get_scan().await?;
//!     for network in scan.iter() {
//!         println!("{} {}", network.signal, network.name);
//!     }
//!     requester.shutdown().await
//! }
//! ```
//!
//! See the examples [on Github](https://github.com/novalabsxyz/wifi-ctrl) for
//! complete station and access-point programs.

#![doc(
    html_logo_url = "https://www.rust-lang.org/logos/rust-logo-128x128-blk.png",
    html_favicon_url = "https://www.rust-lang.org/favicon.ico",
    html_root_url = "https://docs.rs/wifi-ctrl/"
)]
#![doc(test(attr(allow(unused_variables), deny(warnings))))]

use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot};

/// WiFi Access Point runtime and types
pub mod ap;
/// Crate-wide error types
pub mod error;
/// WiFi Station (network client) runtime and types
pub mod sta;

pub(crate) mod config;
pub(crate) mod socket_handle;

use socket_handle::SocketHandle;
pub type Result<T = ()> = std::result::Result<T, error::ClientError>;
pub type SocketResult<T = ()> = std::result::Result<T, error::SocketError>;
pub type ParseResult<T = ()> = std::result::Result<T, error::ParseError>;

use log::{debug, info, warn};

pub(crate) trait ShutdownSignal {
    fn is_shutdown(&self) -> bool;
}
