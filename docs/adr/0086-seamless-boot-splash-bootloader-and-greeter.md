# ADR-0086: Seamless Boot Splash (`Plymouth`), Bootloader Themes & Graphical `greetd` Greeter (`RUSTYOS-157`)

- **Status:** Accepted
- **Date:** 2026-11-23
- **Decision ID:** `D-100`
- **Task:** `RUSTYOS-157`

## Context
Booting RustyOS on both the Live ISO and installed systems exposed three visual and functional gaps between firmware handoff and the desktop session:
1. `systemd-boot` always displayed a text menu for 4 seconds even on single-OS installs, and `GRUB` used default monochrome text without a RustyOS theme.
2. Neither `plymouth` nor quiet kernel parameters (`quiet splash loglevel=3 rd.udev.log_level=3 vt.global_cursor_default=0`) were configured, causing raw kernel and `systemd` logs to scroll during boot and shutdown.
3. `scripts/rustyos-install.sh` still configured text-mode `/usr/bin/agreety` as `greetd`'s `[default_session]`, while the Live ISO's `greetd/config.toml` invoked `rusty-greeter` directly without a Wayland compositor host.

## Decision
1. **Minimalist Plymouth Splash (`packaging/plymouth/rustyos/`):**
   - Add a macOS-inspired `rustyos` Plymouth script theme (`#000000` background, centered white HiDPI RustyOS monogram `logo.png`, rounded monotonic progress bar `progress_bg.png`/`progress_fg.png`, and LUKS password prompt callback).
   - Package it to `/usr/share/plymouth/themes/rustyos/` in `packaging/arch/PKGBUILD`, add `plymouth` + `plymouthd.conf` + `mkinitcpio` `plymouth` hook to both `packaging/archiso/` and `scripts/rustyos-install.sh`.
2. **Bootloader Polish (`systemd-boot` & `GRUB`):**
   - Package a minimalist dark GRUB theme (`packaging/grub/rustyos/theme.txt`, `logo.png`, `select_c.png`, `#0A0A0C` canvas, `#0A84FF` selection accent) to `/usr/share/grub/themes/rustyos/` and wire `GRUB_THEME` + quiet splash kernel parameters in `scripts/rustyos-install.sh`.
   - Use `timeout 0` for single-OS (`whole-disk`) `systemd-boot` installs (`Space` reveals the menu) and `timeout 4` for `dual-boot`, always installing a verbose fallback entry (`rustyos-console.conf` / `RustyOS Live (Console / Verbose)`) without `quiet splash`.
3. **Graphical `greetd` Host Mode (`rusty-compositor --greeter` & `rustyos-session --greeter`):**
   - Add `--greeter <cmd>` to `CompositorConfig` (`crates/rusty-compositor/src/lib.rs`), `greeter_mode: bool` to `RustyCompositorState` (suppressing TopBar, Dock, desktop icons, and user autostart), quote-aware `spawn_greeter_client` with exit-triggered compositor shutdown, and best-effort `plymouth quit --retain-splash` handoff after the first DRM frame.
   - Add `--greeter` mode to `bin/rustyos-session` and configure `[default_session] command = "/usr/bin/rustyos-session --greeter"` in both `packaging/archiso/airootfs/etc/greetd/config.toml` and `scripts/rustyos-install.sh`.

## Consequences
- Fresh installs and Live ISO images transition seamlessly from bootloader → Plymouth splash → `rusty-greeter` / desktop without console flicker or SDDM/Qt dependencies.
- Verbose console boot entries remain one keypress away for hardware debugging.
- Zero new Cargo dependencies and zero modifications to the developer's host OS configuration.
