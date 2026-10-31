# Lightweight Task Manager

A lightweight, friendly, cross-platform task manager for **Windows, macOS and Linux**, built with **Tauri 2**, **Rust** and **Svelte 5**.

The goal is simple: a task manager that is genuinely light, easy to understand and friendly to non-expert users. The Rust core keeps memory use low, and the app only refreshes data while its window is visible.

## Features

- **Processes** – grouped by application with real icons and names. CPU, memory and disk are shown as a heat map. Expand a group to see its child processes, then end the task or toggle efficiency mode.
- **"Why is my PC slow?"** – a plain-language diagnosis that names the top three culprits and gives advice for each.
- **Safe by design** – multi-level warnings before ending critical system processes (`csrss`, `lsass`, `svchost`, `explorer`, …).
- **Details** – a flat per-process table with PID, status, user, memory, 32/64-bit platform and priority. Set priority or toggle efficiency mode per process.
- **Performance** – live CPU, memory, disk and network charts, plus per-core usage.
- **Services** – list, start, stop and restart services (Windows SCM, Linux systemd).
- **Startup apps** – enable or disable apps that start with the system.
- **Security** – scans services for suspicious signs (unusual install paths, system-name impersonation, missing or invalid digital signature, risky command lines) with a one-click VirusTotal lookup.
- **Efficiency mode** – lowers a process's priority and enables power throttling (Windows) or sets the lowest `nice` value (Linux/macOS).
- **Tray mini-widget** – click the tray icon for a small always-on-top view of CPU, RAM, disk, network and the busiest app.
- **Quick search** – press `Ctrl+K` inside the app to jump to a tab, a process or a service.
- **Settings** – refresh interval, light/dark theme, close-to-tray behaviour, start with the system.

> **Note on the Security tab:** it is a heuristic check, not an antivirus. "Suspicious" means "worth a closer look", not "confirmed malware". Legitimate software can be flagged too.

## Platform support

| Feature | Windows | Linux | macOS |
|---|---|---|---|
| Processes, details, performance | ✅ | ✅ | ✅ |
| App icons and display names | ✅ | ⬜ letter placeholder | ⬜ letter placeholder |
| Priority and efficiency mode | ✅ | ✅ (`nice`) | ✅ (`nice`) |
| Services | ✅ | ✅ (systemd) | ⬜ not yet |
| Startup apps | ✅ | ✅ (`~/.config/autostart`) | ⬜ not yet |
| Digital signature check | ✅ | – | – |

Windows is the most complete and the one tested by hand. The Linux build compiles (checked in a Debian container) but has not been run on a real machine. macOS is not built or tested yet; the CI workflow is the first attempt.

## Tech stack

| Layer | Choice |
|---|---|
| App shell | [Tauri 2](https://tauri.app) |
| Backend | Rust, [`sysinfo`](https://crates.io/crates/sysinfo), [`windows`](https://crates.io/crates/windows) / `winreg` on Windows, `libc` on Unix |
| Frontend | Svelte 5, Vite |
| Charts | Hand-rolled SVG sparklines (no chart library) |

## Getting started

### Prerequisites

- [Node.js](https://nodejs.org) 20 or newer
- [Rust](https://rustup.rs) (stable)
- Platform dependencies for Tauri:
  - **Windows:** Microsoft C++ Build Tools and WebView2 (preinstalled on Windows 10/11)
  - **macOS:** `xcode-select --install`
  - **Linux (Debian/Ubuntu):**
    ```bash
    sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
    ```

### Run in development

```bash
npm install
npm run tauri dev
```

The first Rust build takes a few minutes; later builds are incremental.

### Build an installer

```bash
npm run tauri build
```

Bundles are written to `src-tauri/target/release/bundle/`.

## Good to know

- **Administrator rights.** Ending system processes, controlling services and changing priority of protected processes need elevated rights. The Settings tab shows the current state and offers "Restart as administrator" in release builds. In development, start your terminal as administrator and run `npm run tauri dev`.
- **Close to tray.** By default, closing the window hides the app to the system tray so the mini-widget keeps working. Use the tray menu to quit, or change this in Settings.
- **Efficiency mode is not persistent.** It applies to the running process only and resets when that process restarts.
- **`Ctrl+K`** works while the app window is focused; it is not a global shortcut.

## Project structure

```
src/                     Svelte frontend
  App.svelte             app shell and navigation
  lib/                   views, shared state and helpers
src-tauri/
  src/lib.rs             commands and app setup
  src/appmeta.rs         app icons and display names
  src/services.rs        service list and control
  src/procctl.rs         priority and efficiency mode
  src/security.rs        heuristic service scan
  src/startup.rs         startup apps and app autostart
  src/admin.rs           elevation check and relaunch
  src/tray.rs            tray icon, mini-widget, close-to-tray
.github/workflows/       CI (Windows, macOS, Linux)
```

## Continuous integration

`.github/workflows/build.yml` builds the app on Windows, macOS and Ubuntu for every push to `main` and uploads the bundles as artifacts. It can also be started manually from the Actions tab.

## Roadmap

- Vietnamese/English localization
- History for "Why is my PC slow?"
- Proactive notifications when the machine is overloaded
- Per-process network usage and GPU/temperature
- Full macOS support (services, startup apps, icons)
- Code signing and auto-update
- Unit tests for the security scoring and critical-process logic

## License

Released under the [MIT License](LICENSE).
