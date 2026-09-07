# Architecture

AppShelf is a new implementation: QML/Qt Quick on Quickshell, a Rust backend, and a pinned upstream uruntime. There is no Python or web runtime dependency.

It manages two kinds of thing, and the difference runs through the whole design. An AppImage is a single file AppShelf can copy, own and launch itself, so it becomes a managed copy on a shelf with its own launcher and launch settings. A system package is a tree of absolute paths belonging to the distribution's package manager, so AppShelf reads it, converts it if it has to, and hands the transaction to pacman — it never owns one. Identifiers keep the two apart: a managed AppImage is 64 hexadecimal characters of content hash, a package is `pkg:<name>`.

## Processes

`appshelf` resolves its resources and launches Quickshell. The QML `Process` starts the same Rust binary with `--backend`, communicating through JSON lines. The backend serializes requests. Closing the window requests a graceful shutdown after any current mutation finishes.

Desktop launchers call `appshelf --launch <sha256>`, so launching outside the UI uses the same environment and isolation settings. App processes detach from the UI lifetime. The first 300 ms of startup are checked for failure; later diagnostics remain in the app's log.

## Runtime and inspection

The ELF section table identifies where the runtime ends; filesystem detection starts beyond it. This avoids false SquashFS signatures inside runtime code. The supported initial scope is little-endian type-2 AppImages containing SquashFS or DwarFS.

AppShelf verifies the pinned uruntime SHA-256 before use. It reads metadata through uruntime's embedded filesystem tools. Only bounded stdout is consumed; archive member paths are never extracted onto the host during inspection. DwarFS metadata travels through a small ustar reader that accepts regular files. Filesystem tools have a 15-second deadline and output limits.

On explicit launch, `TARGET_APPIMAGE` points at the managed image and `--appimage-extract-and-run` forces the FUSE-free route. The bundled application is executed only then. This requires temporary extraction space and does not make an untrusted application safe.

## Managed storage

The image is copied into a staging directory, identified by SHA-256, and published under `apps/<sha256>`. Its desktop entry is exclusively and atomically created. Mutations share an advisory filesystem lock across backend instances. Configuration writes use a private temporary file and atomic rename.

Uninstall moves only `record.json`, `app.AppImage` and `icon.png` into a temporary removal directory before deleting them and the owned launcher. Other files and per-app isolated data are preserved. A failure before launcher removal rolls those files back. A power loss or SIGKILL can leave `.install-*`/`.remove-*` directories; automatic crash recovery is a future milestone.

## System packages

Arch, Debian and RPM packages are recognised by content where the format has any — an RPM's magic, a `.deb`'s first `ar` member — and by name where it has none, since an Arch package is an ordinary compressed tarball. Metadata is read without the foreign distributions' tools: a `.deb`'s control file through `bsdtar`, and an RPM's lead, signature header and header proper parsed directly, which is a few hundred bytes of the file and never the payload.

Conversion writes a `.PKGINFO` and copies the payload from the source archive into a new `.pkg.tar.zst` with `bsdtar`'s `@archive` source. Nothing is unpacked to the filesystem, so modes the calling user could not restore — setuid bits, root ownership — survive exactly. Path substitutions move `/bin`, `/sbin`, `/lib`, `/lib64`, `/usr/sbin` and `/usr/lib64` onto Arch's usr-merged layout, which pacman would otherwise refuse to write through, and neutralise any `..` component so a payload cannot name a path above its own root. Versions are normalised to pacman's grammar — one release separator, an optional numeric epoch, and `~` folded to `_` — with the original kept alongside so nothing is rewritten out of sight.

No dependency translation is attempted. Debian and RPM name their requirements from their own archives, and a guess would make pacman refuse a package that is really fine; the converted package declares none, and the original list is shown in the window instead. Maintainer scripts are discarded rather than run as root against another distribution's assumptions.

Installation and removal run `sudo pacman` inside a terminal emulator. pacman needs root and asks questions AppShelf cannot answer on the user's behalf, so the backend writes a small script, opens a terminal on it, and waits — on a status file the script's `EXIT` trap always writes, not on the terminal's own exit, because some terminals fork and because closing the window has to be reported as the refusal it is. A registry at `appshelf/packages.json` records only which packages arrived through AppShelf; pacman's database remains the authority on what is installed, and an entry pacman no longer knows is dropped rather than argued with.

## Discovery

Discovery is bounded and read-only, and it looks for AppImages only: a package file on disk is not an installed application, and reporting one as found would be a different claim. It checks common install directories plus absolute file arguments in desktop `Exec`/`TryExec` entries. It identifies images by content, including extensionless installations, and deduplicates canonical paths. It does not execute shell wrappers or scan the entire home directory.

Import creates a managed copy and preserves the old installation, launcher, and settings. The original source is hidden from subsequent discovery while its path and modification timestamp still match the import record. Existing external settings are not migrated automatically. Files placed in arbitrary directories behind opaque scripts can be added manually.

## Omarchy and Flea

The UI imports the installed Omarchy `Commons` module through a local symlink. Active `colors.toml` and `shell.toml` feed its `Color` and `Style` singletons. Theme-name notifications and a two-second fallback reload cover directory replacements; Omarchy owns font resolution and style scale.

AppShelf's desktop entry claims four types: `application/vnd.appimage`, `application/vnd.debian.binary-package`, `application/x-rpm`, and `application/x-alpm-package`, which AppShelf contributes to the shared MIME database itself. That last one needs both a glob weight above the default and a `sub-class-of` for each compression the format uses: a `.pkg.tar.zst` sniffs as `application/zstd`, and a desktop resolving several matching globs keeps only the candidates descended from what it sniffed, so without that ancestry the weight is never consulted.

Flea recognises the same four kinds itself, in `src/appshelf.rs`, and spawns `appshelf <path>` rather than going through `gio open` — so Enter or a double-click on any of them opens the install preview whether or not the associations are claimed. The reverse route calls `flea --gui <directory>` with an argv array; other file managers use `xdg-open` when Flea is unavailable. For a package, the directory revealed is where its desktop entry or its files live, since it has no folder of AppShelf's own.
