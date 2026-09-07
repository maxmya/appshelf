<div align="center">

<img src="packaging/org.omarchy.appshelf.svg" width="80" alt="AppShelf icon">

# AppShelf

**Your applications. One shelf. Your keyboard.**

A small application manager built for Omarchy, with a Quickshell interface and a Rust backend.
It installs AppImages onto a shelf of its own, and Arch, Debian and RPM packages onto your system
through pacman.

[![Rust](https://img.shields.io/badge/backend-Rust-dea584?logo=rust)](Cargo.toml)
[![Quickshell](https://img.shields.io/badge/interface-Quickshell-798186)](https://quickshell.org)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

[Get started](#get-started) · [System packages](#system-packages) · [Updates](#keeping-applications-updated) · [Keyboard controls](#keyboard-controls) · [Command reference](#command-reference) · [Troubleshooting](#troubleshooting) · [How it works](docs/architecture.md)

</div>

AppShelf opens any local application file and shows you what it is before anything is installed. AppImages become managed copies on its own shelf, with their own launcher entries and launch settings. Arch, Debian and RPM packages go to pacman, converted first if they need it, in a terminal you answer yourself. It follows your Omarchy theme and works with [Flea](https://github.com/thisisgm/flea) out of the box once you enable the file association.

## What it opens

| File | What happens |
|---|---|
| `.AppImage`, or any type-2 AppImage | A managed copy on the shelf, with its own launcher and launch settings |
| `.pkg.tar.zst`, `.pkg.tar.xz`, `.pkg.tar` | Handed to `pacman -U` as it is |
| `.deb` | Converted to an Arch package, then handed to `pacman -U` |
| `.rpm` | Converted to an Arch package, then handed to `pacman -U` |

Extensions are a hint, not the rule: an AppImage, a `.deb` and an `.rpm` are all recognised by their content, so a download that lost its extension still works.

## What it does

- **SquashFS and DwarFS.** Both AppImage formats use a pinned, checksum-verified [uruntime](https://github.com/VHSgunzo/uruntime).
- **No libfuse2 requirement.** Apps launch through uruntime's extract-and-run path, with no FUSE mount required.
- **Packages without an AUR helper.** `.deb` and `.rpm` files are converted with `bsdtar`, which pacman already depends on. No `debtap`, no `rpmextract`, no database to rebuild first.
- **pacman stays in charge.** Every install and removal runs in a terminal, with pacman's real output and your own password. AppShelf never answers a prompt for you.
- **Updates from the application's own metadata.** An AppImage that publishes an `.upd_info` descriptor can be checked and updated in place, keeping its name, environment and isolated data.
- **A tray that watches.** An optional StatusNotifierItem rechecks managed AppImages every four hours and notifies you by name when one is behind.
- **AppShelf updates itself.** The released AppImage carries the same kind of descriptor, so the manager is just another AppImage as far as the update code is concerned.
- **Keyboard first.** Navigate with arrows or `j` / `k`, search with `/`, launch with `Enter`, and reach every control with `Tab`.
- **Omarchy styling.** Colors, typography and spacing come from Omarchy's shared Quickshell components. Theme changes apply while AppShelf is open.
- **Find what you already installed.** Discover AppImages from other managers, including extensionless images referenced by desktop launchers.
- **Per-app settings.** Store literal environment variables and choose shared, separate config/data, or separate home/config directories.
- **Flea integration.** Open an AppImage or a package from Flea to review and install it; reveal an app's files from AppShelf.
- **Predictable ownership.** Original downloads and personal application data survive uninstall.

This is an early local application manager. Updates are checked for you but never installed behind your back, and full migration of another manager's settings is not implemented yet.

## Get started

The quickest route is the released AppImage: make it executable and run it. AppShelf recognises that it is running from its own AppImage and opens a setup window instead of the shelf, offering to install itself into `~/.local/bin` and your application launcher, with an optional tick to become your AppImage opener. "Open without installing" runs the shelf straight from the image for a look around.

```bash
chmod +x AppShelf-*-x86_64.AppImage
./AppShelf-*-x86_64.AppImage
```

Installing matters because the copy inside an AppImage lives on a mount that disappears when the process exits — desktop entries and the tray need a copy that stays put. Keep the AppImage afterwards: it is what `appshelf --self-update` replaces.

### From source

You need an Omarchy installation with the Quickshell `Commons` components (developed on Omarchy 4.0.2 / Quickshell 0.3.1), Rust/Cargo, `curl`, `xdg-utils`, `desktop-file-utils`, `shared-mime-info`, and `libarchive` for `bsdtar`. System packages also need `pacman` and a terminal emulator, both of which an Arch system already has. Flea is optional. Supported runtime architectures are x86_64 and aarch64; the current test machine is x86_64.

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

`--install` makes a self-contained user-local installation. `--integrate` makes AppShelf the opener for AppImages and for `.pkg.tar.*`, `.deb` and `.rpm` files, including from Flea, and saves whatever opened AppImages before. Omit `--integrate` to keep your existing file opener; `--no-integrate` hands a previously claimed association back.

`--install` also adds `application/x-alpm-package` to your MIME database. An Arch package is a plain compressed tarball, so without a type of its own the desktop cannot tell one from any other `.tar.zst` and could not offer to open it.

To restore that association later:

```bash
appshelf --restore-association
```

`appshelf --shelf` forces the full shelf even when run from the AppImage, and `appshelf --setup-state` prints what the setup window reads: the installed version, the linked binary and the current AppImage association.

## System packages

Open a `.pkg.tar.zst`, `.pkg.tar.xz`, `.deb` or `.rpm` the same way you open an AppImage — `Ctrl+O`, a drop onto the list, or `Enter` in Flea. AppShelf reads the package's own metadata, tells you what it will install and what it will replace, then runs the transaction.

**The transaction runs in a terminal.** pacman needs root and it asks real questions — about files that already exist, about dependencies it would pull in. Answering those from a window that cannot show them is how a package manager breaks a system, so AppShelf opens your terminal, runs `sudo pacman -U`, and waits for the answer you give it. Closing the window is a refusal, and AppShelf reports it as one.

The terminal is the desktop's own choice where there is one: `xdg-terminal-exec` first, then `alacritty`, `ghostty`, `foot` and `kitty`. AppShelf waits on a status file the transaction script always writes rather than on the terminal process, because some terminals fork and return immediately. A transaction still open after 45 minutes is given up on, so a window you walked away from does not hold the shelf for the rest of the session.

Removing a package is the same: `Delete` on the row runs `sudo pacman -Rns` in a terminal.

### What conversion does, and does not do

`.deb` and `.rpm` files are repacked into an Arch package before pacman sees them. `bsdtar` copies the payload from one archive straight into the other, so nothing is unpacked onto your disk and file modes AppShelf could not otherwise restore — setuid bits, root ownership — cross over exactly as they were. Paths are moved onto Arch's layout on the way: `/bin`, `/sbin`, `/lib`, `/lib64`, `/usr/sbin` and `/usr/lib64` are symbolic links here, and pacman refuses to write through one.

Two things are deliberately left out, and the install window says so before you commit:

- **Dependencies are not translated.** A `.deb` asks for `libc6` and an `.rpm` for `libc.so.6()(64bit)`; neither means anything to pacman, and there is no honest general mapping onto Arch's names. Guessing would make pacman refuse packages that are really fine, so the converted package declares no dependencies and the original list is shown to you instead. If the application will not start, that list is where to look.
- **Maintainer scripts are not run.** A `.deb`'s `postinst` and an `.rpm`'s scriptlets are written for another distribution's filesystem and are discarded rather than executed as root on yours.

A converted package keeps its upstream name. If a repository package shares that name, AppShelf warns you: installing yours replaces it, and the next `pacman -Syu` will replace it back.

To see what pacman would be handed without installing anything:

```bash
appshelf --inspect ~/Downloads/thing.deb
appshelf --convert ~/Downloads/thing.deb ~/Downloads
pacman -Qip ~/Downloads/thing-*.pkg.tar.zst
```

### What AppShelf tracks

pacman owns installed packages; AppShelf only records which of them arrived through it, in `$XDG_DATA_HOME/appshelf/packages.json`, so the shelf can list them without claiming every package on your machine. Remove one with `pacman -R` directly and AppShelf forgets it the next time it refreshes — pacman is the authority, and a shelf that argued with it would only ever be wrong.

Updates for these are pacman's job. AppShelf does not check them, and the tray skips them.

## Keeping applications updated

An AppImage can carry an update descriptor in its `.upd_info` ELF section, put there by whoever built it. AppShelf reads that descriptor and follows it; it never guesses where an application came from.

| Descriptor | How the check is answered |
|---|---|
| `gh-releases-zsync\|owner\|repo\|tag\|pattern` | GitHub's release API, with the pattern matched by glob against the release assets |
| `zsync\|https://…` | The `.zsync` file's own header |
| Anything else | Reported as an unsupported descriptor, with its text shown |

Where a `.zsync` file is published, its header carries a checksum and length for the release file, so AppShelf compares those against the copy on your shelf: a rebuilt release under an unchanged version is still seen as new, and a version bump that changes nothing is not. Where no zsync file exists, the release tag is the only signal available and is used instead.

Nothing is downloaded during a check — a release listing, and at most the first two kilobytes of a `.zsync` file. An AppImage with no descriptor is reported as unsupported, not as out of date; that is a fact about how it was built, not a problem with the app.

- **One application:** select it and press `Ctrl+U`, or run `appshelf --update ID`.
- **All of them:** open settings with `Ctrl+,` and choose *Check all applications*, or run `appshelf --check-update` with no ID.

Installing an update is always your decision. The new image is downloaded to a staging directory beside the shelf, verified as an AppImage, and published under its own content hash; the record's name, environment variables, isolation mode and isolated data directory move across with it, and the previous copy and launcher are removed only once the replacement is in place. Because the identifier is the content hash, an updated application has a new one, and its desktop entry is rewritten to match.

System packages are pacman's. AppShelf does not check them, and the tray does not count them.

## Background service and system tray

AppShelf can sit in your system tray as a StatusNotifierItem and check managed AppImages on its own schedule: once when it starts, then every four hours. When something is behind it sends a desktop notification naming the applications, marks itself as needing attention, and lists them in its tooltip; the icon stays AppShelf's own, since a swapped glyph reads as a different program. Its menu opens the shelf, repeats the check on demand, and quits the tray. It also notices installs, removals and updates made from the shelf window, so its count stays honest without the two processes having to talk to each other.

```bash
appshelf --tray            # run the tray in this terminal
appshelf --enable-service  # systemd user unit and autostart entry, started now
appshelf --service-status  # what is installed, plus systemctl's own status
appshelf --disable-service # stop it and remove both
```

`--enable-service` writes `~/.config/systemd/user/appshelf-tray.service` and `~/.config/autostart/org.omarchy.appshelf.tray.desktop`, then enables and starts the unit. Both are written because a session that honours one does not necessarily honour the other. Opening the shelf window also starts the tray if it is not already up.

## Settings

Press `Ctrl+,` in the shelf for AppShelf's own settings — the running version, how many AppImages and packages it is responsible for, and the controls that are not about a single application:

- **Tray.** Start or stop it now, and choose whether it returns at login.
- **Updates.** Sweep every managed application in one pass.
- **AppShelf itself.** Check for a new release, and install it.
- **Paths.** The command, program directory and data directory this session is using.

### Updating AppShelf itself

The released AppImage embeds the same kind of `.upd_info` descriptor AppShelf reads for the applications it manages, so it updates itself with its own machinery. What an update does depends on how this copy arrived, which settings calls the channel:

| Channel | This build is | An update |
|---|---|---|
| `appimage` | Running from the released AppImage | Replaces that AppImage in place |
| `installed` | The copy `--install` left in `~/.local/share/appshelf/program` | Downloads the release and runs its `--install` |
| `unmanaged` | Built from source or installed from a package | Does nothing — update it the way you installed it |

```bash
appshelf --self-check-update
appshelf --self-update
```

This is why the AppImage is worth keeping after setup: on the `appimage` channel it is the file that gets replaced.

## Your first app

1. Press **Ctrl+O**, drop a local file into the list, or open one from Flea.
2. Review it and choose **Install** (`Ctrl+Enter`). An AppImage becomes a managed copy and a launcher entry, with the original file left where it is. A package goes to pacman in a terminal.
3. For an AppImage, press **Ctrl+E** to configure environment variables or isolation before its first launch.
4. Press **Enter** to launch. For a managed AppImage the same settings apply from Omarchy's launcher; for a package, AppShelf starts the desktop entry the package installed.

AppShelf inspects files without executing anything inside them. Clicking **Launch** runs the app, and installing a package runs pacman as root; install only applications you trust.

## Keyboard controls

Press **F1** at any time on the main screen for the in-app guide.

| Key | Action |
|---|---|
| `↑` / `↓` or `j` / `k` | Navigate applications |
| `Home` / `End` | First / last application |
| `Enter` | Launch an installed app, or review a discovered one |
| `/` or `Ctrl+F` | Search |
| `Ctrl+L` | Return focus to the application list |
| `Ctrl+O` | Choose an AppImage or a package |
| `Ctrl+R` | Refresh and rescan |
| `Ctrl+E` | Edit the selected AppImage's launch settings |
| `Ctrl+Shift+F` | Show the selected app's files in Flea |
| `Delete` | Uninstall an AppImage, or remove a package |
| `Ctrl+Enter` | Install from the preview |
| `Ctrl+S` | Save launch settings |
| `Ctrl+U` | Check for updates (AppImages only) |
| `Ctrl+B`, `Ctrl+\` or `F4` | Toggle side panel |
| `Ctrl+,` | Open AppShelf's settings |
| `Ctrl +` / `Ctrl -` | Zoom in / out |
| `Ctrl 0` | Reset zoom |
| `Tab` / `Shift+Tab` | Move between controls |
| `Space` / `Enter` | Activate a focused button |
| `Escape` | Close a dialog or clear list search |
| `Ctrl+Q` | Quit after any current operation finishes |

## Bring your existing apps

AppShelf scans at startup and when you press `Ctrl+R` for every format it can install — AppImages, and Arch, Debian and RPM packages. It checks:

- `~/Downloads`, `~/Desktop` (or wherever `user-dirs.dirs` puts them), `~/Applications`, `~/AppImages`, `~/.local/bin`, `~/.local/opt`, `$XDG_DATA_HOME/appimages` and `/opt`.
- User and system desktop entries, following absolute file arguments in `Exec` and `TryExec`.
- File content rather than file names, so the `.AppImage` extension is not required and a download saved as `foo.deb.1` is still recognised.

A package file is listed under the name and version it declares, not the name of the file. One that pacman already has at that version is not offered again.

Discovered apps are marked **Found**. Select one and press **Enter** to add an AppImage to the shelf, or to hand a package to pacman. The shelf icon fills up while a scan is running.

**Settings → Scanning** switches the whole thing off. With it off nothing is walked, no desktop entries are read and the shelf shows only what it manages and what pacman installed for it. The choice is kept in `$XDG_DATA_HOME/appshelf/settings.json`.

### Hiding what you do not want offered

Press **Ignore** in the side panel to leave a found file out of scanning. Nothing is deleted or moved — the file stays exactly where it is, and the shelf simply stops listing it. The count of ignored files appears beside the section heading; **Show N ignored** brings them back into the list, where **Stop ignoring** undoes it. Ignored paths are kept in `$XDG_DATA_HOME/appshelf/ignored.json`, and an entry whose file has gone is dropped on its own.

The **Found on your computer** heading folds the whole section away when you click it, which is worth doing on a machine with a busy Downloads folder.

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
| `appshelf/packages.json` | Which system packages AppShelf installed, and where each came from |
| `appshelf/ignored.json` | Found files you asked the shelf to stop offering |
| `appshelf/settings.json` | AppShelf's own preferences, such as whether to scan at all |
| `appshelf/data/<sha256>/` | Optional isolated home/config/data/cache/state |
| `appshelf/logs/<sha256>.log` | Output from the most recent launch |
| `applications/org.omarchy.appshelf.app.<sha256>.desktop` | Managed app launcher |

Uninstall removes only AppShelf's managed image, record, icon and launcher. The original source, isolated data and other files placed beside an image are retained. Exact duplicate AppImages are rejected by content hash.

Converted packages are built under `$XDG_CACHE_HOME/appshelf/convert/` and deleted once pacman has finished with them. Nothing about an installed package lives in AppShelf's own storage; pacman's database is the record.

## Command reference

Everything the window does is reachable from the command line, because the window drives the same binary.

| Command | What it does |
|---|---|
| `appshelf` | The shelf — or, when run from the released AppImage, the first-run setup window |
| `appshelf FILE` | Review and install a local file or a `file://` URL, in the compact installer window |
| `appshelf --shelf` | The full shelf, even when run from the AppImage |
| `appshelf --launch ID` | Launch a managed application with its saved settings; this is what desktop entries call |
| `appshelf --scan` | Managed and discovered applications, as JSON |
| `appshelf --inspect FILE` | What AppShelf reads from a file, without installing anything |
| `appshelf --convert FILE [DIR]` | Write the converted Arch package and print its path |
| `appshelf --check-update [ID]` | Check one application, or every one of them |
| `appshelf --update ID` | Download and install an application's update |
| `appshelf --self-check-update`, `--self-update` | The same two, for AppShelf itself |
| `appshelf --tray` | Run the tray in the foreground |
| `appshelf --enable-service`, `--disable-service`, `--service-status` | The tray's background service |
| `appshelf --install [--integrate\|--no-integrate]` | Install into `~/.local/bin` and the application launcher |
| `appshelf --setup-state` | What the setup window reads, as JSON |
| `appshelf --restore-association` | Hand the AppImage association back to whatever held it before |
| `appshelf --fetch-runtime` | Download and verify the pinned uruntime |
| `appshelf --backend` | The JSON-lines backend the QML window speaks to |
| `appshelf --version`, `--help` | Version and usage |

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

In a checkout, `./appshelf` builds and runs the debug binary against the checkout's own `ui/`, so there is no need to install first.

### Where the code lives

| Path | Responsibility |
|---|---|
| `src/main.rs` | Argument handling, the JSON-lines backend loop, and launching Quickshell |
| `src/manager.rs` | The managed shelf: install, configure, launch, uninstall, records and launchers |
| `src/runtime.rs` | uruntime, ELF and SquashFS/DwarFS inspection, metadata and icon extraction |
| `src/package.rs` | Package detection, metadata, conversion, and pacman transactions in a terminal |
| `src/update.rs` | `.upd_info` descriptors, release resolution, and applying an application's update |
| `src/selfupdate.rs` | The same machinery pointed at AppShelf's own releases |
| `src/discovery.rs` | Finding AppImages already installed by something else |
| `src/install.rs` | AppShelf's own installation, MIME types and file associations |
| `src/service.rs`, `src/tray.rs` | The systemd user unit, autostart entry and the tray itself |
| `ui/shell.qml` | The shelf window, its settings view and the IPC handler |
| `ui/install.qml`, `ui/setup.qml` | The compact installer, and the first-run setup window |
| `scripts/`, `packaging/`, `PKGBUILD` | AppImage build, desktop and MIME files, and a local Arch package recipe |

### Building a release

```bash
cargo build --release --locked
./target/release/appshelf --fetch-runtime
scripts/build-appimage.sh dist
```

**Every push to `main` publishes a release.** The `Release` workflow takes the version from `Cargo.toml`: if that tag is free it releases exactly that version, and otherwise it walks the patch number forward to the first free tag, commits the bump back to `main` and releases that. So a deliberate `scripts/version.sh set 0.7.0` in your own commit is honoured as a minor or major release, and a push that says nothing about versions still ships as the next patch. The bump commit is pushed with the default `GITHUB_TOKEN`, which by design starts no further workflow run, so this cannot loop.

The script packs the release binary with `ui/`, `vendor/` and `packaging/` behind the same vendored uruntime AppShelf uses for managed applications, patches the `.upd_info` descriptor that makes `--self-update` work, and writes the `.zsync` and `.sha256` files a release needs beside it. `zsyncmake` is required; set `APPSHELF_SKIP_ZSYNC=1` for a local build that is not going to be released. `makepkg -si` builds the same tree as a local Arch package — it is not an AUR package.

`tests/packages.rs` builds a `.deb` and an `.rpm` byte by byte — including the RPM lead and header AppShelf parses without the `rpm` tools — converts them, and checks the result with `pacman -Qip` and `pacman -Qlp`, which read a package file and need no privileges. It verifies that setuid bits and root ownership survive conversion, that pre-usr-merge paths are moved, and that a payload naming a path above its own root cannot escape. Nothing in the suite installs a package or opens a terminal.

Useful diagnostics:

```bash
appshelf --scan
appshelf --inspect ~/Downloads/thing.deb
appshelf --convert ~/Downloads/thing.deb /tmp
quickshell ipc -p ~/.local/share/appshelf/program/ui call appshelf state
```

The [architecture notes](docs/architecture.md) describe the process boundary, storage and runtime decisions. The [implementation plan](docs/plan.md) tracks the initial scope and next milestones.

## Troubleshooting

| What you see | What it means |
|---|---|
| `Omarchy Quickshell Commons components are required` | The shared components are missing, or `OMARCHY_PATH` points elsewhere. AppShelf links them from `$OMARCHY_PATH/shell/Commons`, defaulting to `/usr/share/omarchy` |
| `No terminal emulator was found to run pacman in` | Install `xdg-terminal-exec`, or one of `alacritty`, `ghostty`, `foot`, `kitty` |
| `The terminal closed before pacman could run` | The terminal exited without leaving a status behind, usually because it did not accept the arguments it was given |
| `pacman refused the transaction; its terminal shows why` | pacman said no — a conflicting file, an unmet dependency, an unknown key. Its own output is the answer |
| `No embedded update information (.upd_info)` | That AppImage's author published no update descriptor. Nothing is wrong with the application |
| `uruntime supports x86_64 and aarch64 in this release` | The pinned runtime has no build for this machine |
| `An installation with the updated version hash already exists` | The update resolves to an image already on the shelf; remove the duplicate first |
| An application installs but will not start | Read `$XDG_DATA_HOME/appshelf/logs/<sha256>.log` — the output of its most recent launch |

A source checkout needs `--fetch-runtime` once before it can inspect anything: without the runtime there is no way to read an AppImage's filesystem. And an update or install that seems to hang is usually a terminal waiting for an answer somewhere behind the shelf window.

## Current limits

- Local, little-endian type-2 SquashFS/DwarFS AppImages; no type-1 ISO images or arbitrary executables.
- System packages need pacman, so they work on Arch-based systems only. Converted `.deb` and `.rpm` packages declare no dependencies and run no maintainer scripts, so an application may still need libraries installed by hand.
- Packages are installed and removed through a terminal, and AppShelf blocks until that terminal finishes or is closed.
- FUSE-free launching extracts into temporary storage and needs enough free space. Large apps can take longer to start.
- Metadata uses embedded desktop names and PNG icons; unsupported metadata falls back to the filename and an initial. An AppImage's version comes from the `X-AppImage-Version` key in its desktop entry, and from its filename when it declares none — so a build that says neither shows no version at all. It is recorded when the application is installed, not re-read afterwards.
- Updates are checked automatically but installed only when you ask, and only for AppImages that publish an `.upd_info` descriptor. There is no application catalogue to download from, no in-place adoption of another manager's installation, and no automatic migration of external settings. System package updates are pacman's, not AppShelf's.
- An abrupt power loss can leave staging directories; automatic recovery is planned.
- Application compatibility still depends on its bundled dependencies and host architecture.

## Credits and license

Built for [Omarchy](https://omarchy.org) with [Quickshell](https://quickshell.org), integrated with [Flea](https://github.com/thisisgm/flea), and powered by [uruntime](https://github.com/VHSgunzo/uruntime).

Package conversion uses [libarchive](https://libarchive.org)'s `bsdtar`, and installation is [pacman](https://wiki.archlinux.org/title/Pacman)'s work throughout.

AppShelf is an independent project, not an official Omarchy component or an AppManager fork. Its original code is [MIT licensed](LICENSE). Omarchy components are loaded from the installed system; runtime downloads retain their upstream licenses. See [runtime provenance](vendor/README.md).
