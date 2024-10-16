//! Tokio-based runtimes for communicating with hostapd and wpa-supplicant.
//!
//! # Quick Start
//!
//! Checkout the examples [on Github](https://github.com/novalabsxyz/wifi-ctrl) for a quick start.
//!

#![doc(
    html_logo_url = "https://www.rust-lang.org/logos/rust-logo-128x128-blk.png",
    html_favicon_url = "https://www.rust-lang.org/favicon.ico",
    html_root_url = "https://docs.rs/wpactrl/"
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
pub type Result<T = ()> = std::result::Result<T, error::Error>;

use log::{debug, error, info, warn};

pub(crate) trait ShutdownSignal {
    fn is_shutdown(&self) -> bool;
    fn inform_of_shutdown(self);
}
