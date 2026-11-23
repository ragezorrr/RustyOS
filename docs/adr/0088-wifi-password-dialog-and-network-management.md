# ADR-0088: Wi-Fi Password Dialog & Network Management (`RUSTYOS-159`)

- **Status:** Accepted
- **Date:** 2026-11-23
- **Decision ID:** `D-102`
- **Task:** `RUSTYOS-159`

## Context
On a freshly installed RustyOS desktop, connecting to a new WPA2/WPA3 Wi-Fi network from the UI was impossible:
1. `crates/rusty-services/src/connectivity.rs` exposed `NetworkService::connect_wifi_with_password` and `rescan_wifi_networks`, but lacked `disconnect_wifi` and `forget_wifi`.
2. `apps/rusty-settings` (`SettingsPage::Wifi`) only dispatched passwordless `ServiceCommand::ConnectWifi`, showed no authentication error feedback, had no `Disconnect` / `Forget` flows, and did not trigger an active Wi-Fi rescan on `Refresh`.
3. `crates/rusty-compositor` (`TopBar` `WifiPopover`) opened `rusty-settings --page wifi` when clicking an unknown secured access point without forwarding the chosen SSID.

## Decision
1. **`NetworkService` (`crates/rusty-services/src/connectivity.rs`):**
   - Add `disconnect_wifi(&self, ssid: &str) -> Result<(), String>` (`nmcli connection down id <ssid>`) and `forget_wifi(&self, ssid: &str) -> Result<(), String>` (`nmcli connection delete id <ssid>`) with `Stdio::null()` and captured stderr/stdout diagnostics.
2. **Wi-Fi Password Modal & Network Actions (`apps/rusty-settings`):**
   - Add optional CLI flag `--wifi-ssid <ssid>` (implies `--page wifi`) to open the password sheet directly for the selected network.
   - Add `SettingsModal::WifiPassword` (built on `rusty_ui::DialogSheet::prompt` + masked `TextField` + clickable `rusty_ui::Checkbox` `"Show password"`) and `SettingsModal::WifiNetworkInfo` (destructive `"Forget"` confirmation triggered via the row `(...)` button `RowControlId::WifiInfoIndex`).
   - Add `RowControlId::WifiDisconnectActive` under the active connection header, plus `ServiceCommand::{ConnectWifiWithPassword, DisconnectWifi, ForgetWifi, RescanWifi}` and `ServiceSnapshot::wifi_action_result` (`WifiActionOutcome`) so connection errors automatically reopen `SettingsModal::WifiPassword` with the real diagnostic without ever logging passwords.
3. **`TopBar` Integration (`crates/rusty-compositor`):**
   - Add `RustyCompositorState::open_wifi_password_settings_action(&mut self, ssid: &str)` launching `rusty-settings --page wifi --wifi-ssid <ssid>` when clicking an unknown secured network (`ap.secure && !ap.known`) in `WifiPopover`, keeping known/open networks as one-click connects inside `TopBar`.

## Consequences
- Users can join new WPA2/WPA3 networks, toggle password visibility, retry on auth failure, disconnect, forget saved profiles, and trigger live rescans cleanly from `rusty-settings` and `TopBar`.
- Zero new Cargo dependencies, zero IPC protocol changes, and zero compositor keyboard-intercept complexity.
