<div align="center">

<img src="packaging/org.omarchy.appshelf.svg" width="80" alt="AppShelf icon">

# AppShelf

**Your AppImages. One shelf. Your keyboard.**

A small AppImage manager built for Omarchy, with a Quickshell interface and a Rust backend.

[![Rust](https://img.shields.io/badge/backend-Rust-dea584?logo=rust)](Cargo.toml)
[![Quickshell](https://img.shields.io/badge/interface-Quickshell-798186)](https://quickshell.org)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

[Get started](#get-started) · [Keyboard controls](#keyboard-controls) · [Existing apps](#bring-your-existing-apps) · [How it works](docs/architecture.md)

</div>

AppShelf keeps local AppImages organized, adds them to your application launcher, and gives each app its own launch settings. It follows your Omarchy theme and works with [Flea](https://github.com/thisisgm/flea) out of the box once you enable the file association.

## What it does

- **SquashFS and DwarFS.** Both AppImage formats use a pinned, checksum-verified [uruntime](https://github.com/VHSgunzo/uruntime).
- **No libfuse2 requirement.** Apps launch through uruntime's extract-and-run path, with no FUSE mount required.
- **Keyboard first.** Navigate with arrows or `j` / `k`, search with `/`, launch with `Enter`, and reach every control with `Tab`.
- **Omarchy styling.** Colors, typography and spacing come from Omarchy's shared Quickshell components. Theme changes apply while AppShelf is open.
- **Find what you already installed.** Discover AppImages from other managers, including extensionless images referenced by desktop launchers.
- **Per-app settings.** Store literal environment variables and choose shared, separate config/data, or separate home/config directories.
- **Flea integration.** Open an AppImage from Flea to review and install it; reveal an app's folder from AppShelf.
- **Predictable ownership.** Original downloads and personal application data survive uninstall.

This is an early local-app manager. Automatic updates and full migration of another manager's settings are not implemented yet.

## Get started

You need an Omarchy installation with the Quickshell `Commons` components (developed on Omarchy 4.0.2 / Quickshell 0.3.1), Rust/Cargo, `curl`, `xdg-utils` and `desktop-file-utils`. Flea is optional. Supported runtime architectures are x86_64 and aarch64; the current test machine is x86_64.

```bash
git clone https://github.com/maxmya/appshelf.git
cd appshelf
cargo build --release --locked
./target/release/appshelf --fetch-runtime
./target/release/appshelf --install --integrate
```

Open **AppShelf** from your application launcher, or run:

```bash
~/.local/bin/appshelf
```

`--install` makes a self-contained user-local installation. `--integrate` makes AppShelf the opener for type-2 AppImages, including from Flea, and saves your previous association. Omit `--integrate` to keep your existing file opener.

To restore that association later:

```bash
appshelf --restore-association
```

## Background service and system tray

AppShelf can run in your system tray (StatusNotifierItem) and monitor installed AppImages for updates in the background.

To enable the background systemd user service and desktop autostart:
```bash
appshelf --enable-service
```
To run the system tray directly:
```bash
appshelf --tray
```
To check service status or disable:
```bash
appshelf --service-status
appshelf --disable-service
```

For a development checkout, `./appshelf` builds and runs the debug binary. A local Arch package recipe is also included: run `makepkg -si` from the checkout. It is not an AUR package.

## Your first app

1. Press **Ctrl+O**, drop a local AppImage into the list, or open one from Flea.
2. Review the file and choose **Install** (`Ctrl+Enter`). AppShelf keeps the original file and creates a managed copy and launcher entry.
3. Press **Ctrl+E** to configure environment variables or isolation before its first launch.
4. Press **Enter** to launch. The same settings apply when you open the app from Omarchy's launcher.

AppShelf inspects images without executing their bundled runtime. Clicking **Launch** executes the app; install only applications you trust.

## Keyboard controls

Press **F1** at any time on the main screen for the in-app guide.

| Key | Action |
|---|---|
| `↑` / `↓` or `j` / `k` | Navigate applications |
| `Home` / `End` | First / last application |
| `Enter` | Launch a managed app or review a discovered app |
| `/` or `Ctrl+F` | Search |
| `Ctrl+L` | Return focus to the application list |
| `Ctrl+O` | Choose an AppImage |
| `Ctrl+R` | Refresh and rescan installed AppImages |
| `Ctrl+E` | Edit the selected app's launch settings |
| `Ctrl+Shift+F` | Show the selected app in Flea |
| `Delete` | Open uninstall confirmation |
| `Ctrl+Enter` | Install from the preview |
| `Ctrl+S` | Save launch settings |
| `Ctrl+U` | Check for updates |
| `Ctrl+B` | Toggle side panel |
| `Ctrl +` / `Ctrl -` | Zoom in / out |
| `Ctrl 0` | Reset zoom |
| `Tab` / `Shift+Tab` | Move between controls |
| `Space` / `Enter` | Activate a focused button |
| `Escape` | Close a dialog or clear list search |
| `Ctrl+Q` | Quit after any current operation finishes |

## Bring your existing apps

AppShelf scans at startup and when you press `Ctrl+R`. It checks:

- `~/Applications`, `~/AppImages`, `~/.local/bin`, `~/.local/opt`, `$XDG_DATA_HOME/appimages` and `/opt`.
- User and system desktop entries, following absolute file arguments in `Exec` and `TryExec`.
- AppImage content signatures, so the `.AppImage` extension is not required.

Discovered apps are marked **Found**. Select one and press **Enter** to add it to AppShelf.

**Import creates a managed copy.** It does not take over or delete the original image, launcher, settings or manager registry. Both launchers may remain visible until you remove the old installation with its original manager. Configure AppShelf's copy before launching if the old app used special environment or isolation settings.

Discovery is bounded to four directory levels and does not follow directory symlinks. Opaque shell scripts, arbitrary custom directories and fully extracted AppDirs are not automatically migrated. You can add a local AppImage directly with `Ctrl+O`.

## Environment and isolation

Select an app and press **Ctrl+E**. Environment entries use `NAME=value`, one per line:

```text
QT_QPA_PLATFORM=wayland
ELECTRON_OZONE_PLATFORM_HINT=auto
```

Values are literal: AppShelf does not run a shell, expand `$VARIABLES`, or interpret command substitutions. Settings are applied on the next launch.

| Mode | Behavior |
|---|---|
| Shared | Inherit your regular home and XDG application directories |
| Config + data | Separate config, data, cache and state directories; keep your normal home |
| Home + config | Separate home as well as all four XDG directories |

Isolation starts with separate directories; it does not copy your existing settings. Switching back to Shared retains those directories for later use. **This is data isolation, not a security sandbox.** An app can still access files allowed by your user account.

## Where everything lives

Paths below use `$XDG_DATA_HOME`, normally `~/.local/share`.

| Path | Contents |
|---|---|
| `appshelf/program/` | Installed Rust binary, QML and uruntime |
| `appshelf/apps/<sha256>/` | Managed AppImage, metadata record and optional icon |
| `appshelf/data/<sha256>/` | Optional isolated home/config/data/cache/state |
| `appshelf/logs/<sha256>.log` | Output from the most recent launch |
| `applications/org.omarchy.appshelf.app.<sha256>.desktop` | Managed app launcher |

Uninstall removes only AppShelf's managed image, record, icon and launcher. The original source, isolated data and other files placed beside an image are retained. Exact duplicate AppImages are rejected by content hash.

## Development and checks

```bash
cargo build --locked
./target/debug/appshelf --fetch-runtime
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
./appshelf
```

Integration tests construct real SquashFS and DwarFS images, launch controlled fixtures without FUSE, verify environment and isolation, and exercise discovery, duplicate detection, filename escaping and removal ownership. Tests run against temporary application stores.

Useful diagnostics:

```bash
appshelf --scan
quickshell ipc -p ~/.local/share/appshelf/program/ui call appshelf state
```

The [architecture notes](docs/architecture.md) describe the process boundary, storage and runtime decisions. The [implementation plan](docs/plan.md) tracks the initial scope and next milestones.

## Current limits

- Local, little-endian type-2 SquashFS/DwarFS AppImages; no type-1 ISO images or arbitrary executables.
- FUSE-free launching extracts into temporary storage and needs enough free space. Large apps can take longer to start.
- Metadata uses embedded desktop names and PNG icons; unsupported metadata falls back to the filename and an initial.
- No automatic updates, application downloads, in-place adoption or automatic migration of external settings.
- An abrupt power loss can leave staging directories; automatic recovery is planned.
- Application compatibility still depends on its bundled dependencies and host architecture.

## Credits and license

Built for [Omarchy](https://omarchy.org) with [Quickshell](https://quickshell.org), integrated with [Flea](https://github.com/thisisgm/flea), and powered by [uruntime](https://github.com/VHSgunzo/uruntime).

AppShelf is an independent project, not an official Omarchy component or an AppManager fork. Its original code is [MIT licensed](LICENSE). Omarchy components are loaded from the installed system; runtime downloads retain their upstream licenses. See [runtime provenance](vendor/README.md).
