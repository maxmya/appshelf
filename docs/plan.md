# Implementation plan

## First release

- [x] Independent Rust backend and Quickshell/QML UI.
- [x] Omarchy shared style, live palette and font synchronization.
- [x] SquashFS and DwarFS identification and metadata.
- [x] Pinned, verified uruntime and FUSE-free launch path.
- [x] Managed installation, desktop launchers, launch, reveal and removal.
- [x] Per-app literal environment variables and configurable data isolation.
- [x] Existing-AppImage discovery and explicit copy-based import.
- [x] Keyboard navigation, shortcuts, focus restoration and in-app help.
- [x] Flea MIME association and user-local installation.
- [x] Rust integration tests with real SquashFS/DwarFS fixtures.
- [ ] Verified updates and recovery after interrupted updates.
- [ ] Migration of settings from other managers.
- [ ] Optional in-place adoption with conflict handling.
- [ ] Automatic recovery of staging directories after power loss.

The first release focuses on local AppImages. It does not manage pacman/AUR packages, download applications or remove other managers' installations.
