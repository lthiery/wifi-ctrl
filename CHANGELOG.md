# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - Unreleased

### Security
- PSK/BSSID encoding hardened against injection into `SET_NETWORK` commands:
  passphrases are validated as quotable printable ASCII and sent quoted
  (previously a space-containing passphrase was hex-encoded and rejected by
  wpa_supplicant), precomputed 64-hex PSKs are sent raw, and BSSIDs are
  validated as canonical `xx:xx:xx:xx:xx:xx` addresses. Invalid input returns
  the new `ClientError::InvalidPsk` / `ClientError::InvalidBssid`.
- The PSK is redacted from the `SET_NETWORK` debug log, and hostapd `SetValue`
  values (e.g. `wpa_passphrase`) are redacted from request logs.
- Dropped the derived `PartialEq`/`Eq` on `Psk` — a derived comparison over key
  material would not be constant-time.

### Changed
- **Breaking:** split `Error` into two enums, separating internal errors from
  errors reported back to API requesters.
- **Breaking:** `sta::Status` is now a typed struct (common fields typed,
  everything else via `raw`/`get`) instead of a `HashMap` alias; unknown keys
  are preserved. Driver/version-specific `ap::Status` fields are now `Option`
  and the BSS vectors default to empty, so a missing key no longer fails the
  whole parse.
- **Breaking:** `WifiSetupGeneric<C, B>` is gone; `WifiSetup` is now a plain
  struct. Channel capacities default to 32; use `WifiSetup::with_capacities()`
  to construct with explicit request and broadcast channel sizes.
- **Breaking:** renamed `ConfigError::MissingDelimterEqual` to
  `MissingDelimiterEqual`.
- **Breaking:** removed the event channel; events are now handled directly by
  the event socket instead of being forwarded through a channel.
- **Breaking:** moved some errors out of the select result.
- Select timeouts are now polled inside the event loop instead of a spawned
  task, so the library no longer requires tokio's `rt` feature and the timeout
  can no longer queue behind pending requests.
- **Breaking:** upgraded the crate to Rust edition 2024; the MSRV is now
  declared as `rust-version = "1.85"` and checked in CI.
- Pinned loose `"0"` dependency requirements to real minor lines.

### Added
- Control-socket command timeouts on `request()` (previously a reply-less
  response could freeze the runtime, including shutdown handling), configurable
  via `set_command_timeout()` (default 3s).
- The hostapd `ATTACH`/`LOG_LEVEL` retry loops are bounded (~60s, returning the
  new `SocketError::AttachFailed`) and configurable via `set_attach_retries()`
  / `set_attach_retry_delay()`.
- Unicode support for SSIDs and PSKs, including in status responses.
- Command to reload configuration from disk.
- Method for sending a `&str` request to the socket.
- Method for broadcasting a message.
- Doctests, intra-doc links, and a crate-level quick start.

### Fixed
- API error responses are sent back to the requester instead of being dropped.
- `SCAN_RESULTS` is fetched inline on `ScanComplete`, avoiding a self-send
  deadlock.
- `FAIL-BUSY` is only accepted for `SCAN`, not for every command.
- Unparseable scan lines are skipped instead of failing the whole scan.
- `STATUS` parse failures are reported to the `SelectNetwork` requester.
- Scan failures are handled instead of erroring.
- Trailing tabs are no longer stripped off scan results.

## [0.2.5] - 2024-05-22

### Added
- Custom requests for both station and access point.
- Method to set the BSSID for a network.
- Method to remove all networks.
- Unknown station events are broadcast instead of dropped.
- `WifiAp` example program; attach options allowed.
- Example program takes user input for selecting the interface.

### Changed
- Station types implement `Display` instead of `ToString` directly.

### Fixed
- Grow buffer on event sockets.

_Note: 0.2.4 was prepared but never tagged; its changes are included here._

## [0.2.3] - 2023-05-30

### Added
- Support for generic `set` hostapd requests.
- Support for `enable`/`disable` hostapd requests.
- Support for `get_config` hostapd requests.

### Changed
- Documentation improvements.

## [0.2.2] - 2023-05-29

### Added
- Setting of `key_mgmt`.

## [0.2.1] - 2023-04-12

### Added
- Timeout mechanism for select requests, with a `SelectResult::Timeout`
  response.

### Fixed
- If already connected, don't await an event.
- Cancel the timeout when a response is sent.
- Don't error when the request receiver has been dropped.

## [0.2.0] - 2023-03-23

### Changed
- Requests are deferred during startup.

## [0.1.4] - 2023-03-22

### Changed
- `NetworkResult` fields made public.
- Collapsed nested `Result`s in `wifi_sta::get_status`.

## [0.1.3] - 2023-03-20

### Added
- Shutdown supported during startup.

### Changed
- Documentation notes; removed a redundant join.

## [0.1.2] - 2023-01-11

### Fixed
- README link points to the latest version.

## [0.1.1] - 2023-01-05

### Added
- Example, documentation, and `wpa_supplicant` crate keyword.

### Changed
- Internal structs and enums are `pub(crate)` instead of `pub`.
- Removed `anyhow` usage and the `logging` feature.

## [0.1.0] - 2023-01-04

### Added
- Initial release, extracted from a larger project.

[0.3.0]: https://github.com/lthiery/wifi-ctrl/compare/v0.2.5...HEAD
[0.2.5]: https://github.com/lthiery/wifi-ctrl/compare/v0.2.3...v0.2.5
[0.2.3]: https://github.com/lthiery/wifi-ctrl/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/lthiery/wifi-ctrl/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/lthiery/wifi-ctrl/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/lthiery/wifi-ctrl/compare/v0.1.4...v0.2.0
[0.1.4]: https://github.com/lthiery/wifi-ctrl/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/lthiery/wifi-ctrl/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/lthiery/wifi-ctrl/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/lthiery/wifi-ctrl/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/lthiery/wifi-ctrl/releases/tag/v0.1.0
