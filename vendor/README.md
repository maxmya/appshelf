# Runtime provenance

AppShelf downloads the official full AppImage build of [VHSgunzo/uruntime](https://github.com/VHSgunzo/uruntime) **v0.6.1**. The runtime binaries are ignored by Git and are fetched directly from upstream with `appshelf --fetch-runtime`.

The SHA-256 values pinned in `src/runtime.rs` are the asset digests published by the official GitHub release:

| Architecture | SHA-256 |
|---|---|
| x86_64 | `9763e3e6605efa970d98c2e51abd8385f991b5c2df7684858950f4372ff67982` |
| aarch64 | `39b0184ef33d77b694716b8b69e151a8a3492acec6f3ad1cd3ebc18a91792df2` |

`URUNTIME-LICENSE` is the upstream runtime license. Its embedded SquashFS/DwarFS tools have their own upstream licenses. No runtime binary or copied Omarchy/Flea source is published in this repository; Omarchy components are loaded from the user's installation.
