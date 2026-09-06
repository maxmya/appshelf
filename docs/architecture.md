# Architecture

AppShelf is a new implementation: QML/Qt Quick on Quickshell, a Rust backend, and a pinned upstream uruntime. There is no Python or web runtime dependency.

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

## Discovery

Discovery is bounded and read-only. It checks common install directories plus absolute file arguments in desktop `Exec`/`TryExec` entries. It identifies images by content, including extensionless installations, and deduplicates canonical paths. It does not execute shell wrappers or scan the entire home directory.

Import creates a managed copy and preserves the old installation, launcher, and settings. The original source is hidden from subsequent discovery while its path and modification timestamp still match the import record. Existing external settings are not migrated automatically. Files placed in arbitrary directories behind opaque scripts can be added manually.

## Omarchy and Flea

The UI imports the installed Omarchy `Commons` module through a local symlink. Active `colors.toml` and `shell.toml` feed its `Color` and `Style` singletons. Theme-name notifications and a two-second fallback reload cover directory replacements; Omarchy owns font resolution and style scale.

The `application/vnd.appimage` association points to AppShelf's desktop entry. Flea uses `xdg-open`, opening an install preview. The reverse route calls `flea --gui <directory>` with an argv array; other file managers use `xdg-open` when Flea is unavailable.
