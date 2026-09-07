# Build a local package from this checkout: makepkg -f
pkgname=appshelf
pkgver=0.5.0
pkgrel=1
pkgdesc='Omarchy-first application manager for AppImages and .pkg.tar/.deb/.rpm packages, with a Quickshell UI'
arch=('x86_64' 'aarch64')
license=('MIT')
# libarchive is what converts a .deb or .rpm into a package pacman accepts;
# pacman itself pulls it in, but AppShelf depends on `bsdtar` by name.
depends=('quickshell' 'omarchy' 'desktop-file-utils' 'xdg-utils' 'libarchive' 'shared-mime-info' 'pacman')
makedepends=('cargo' 'curl')
optdepends=('flea: reveal managed applications and open packages from the file manager'
            'xdg-terminal-exec: choose the terminal pacman transactions open in')
install=packaging/appshelf.install

build() {
  cd "$startdir"
  cargo build --release --locked
  target/release/appshelf --fetch-runtime
}

check() {
  cd "$startdir"
  cargo test --locked
}

package() {
  install -d "$pkgdir/usr/share/appshelf" "$pkgdir/usr/bin"
  cp -r "$startdir/ui" "$startdir/vendor" "$startdir/packaging" "$pkgdir/usr/share/appshelf/"
  install -m755 "$startdir/target/release/appshelf" "$pkgdir/usr/share/appshelf/appshelf"
  ln -s /usr/share/appshelf/appshelf "$pkgdir/usr/bin/appshelf"
  ln -sfn /usr/share/omarchy/shell/Commons "$pkgdir/usr/share/appshelf/ui/Commons"
  install -Dm644 "$startdir/packaging/org.omarchy.appshelf.desktop" "$pkgdir/usr/share/applications/org.omarchy.appshelf.desktop"
  install -Dm644 "$startdir/packaging/org.omarchy.appshelf.svg" "$pkgdir/usr/share/icons/hicolor/scalable/apps/org.omarchy.appshelf.svg"
  install -Dm644 "$startdir/packaging/org.omarchy.appshelf.png" "$pkgdir/usr/share/icons/hicolor/512x512/apps/org.omarchy.appshelf.png"
  # An Arch package has no MIME type of its own in the shared database; this
  # adds one so `.pkg.tar.zst` can be opened rather than merely uncompressed.
  install -Dm644 "$startdir/packaging/org.omarchy.appshelf.mime.xml" "$pkgdir/usr/share/mime/packages/org.omarchy.appshelf.xml"
  install -Dm644 "$startdir/LICENSE" "$pkgdir/usr/share/licenses/appshelf/LICENSE"
}
