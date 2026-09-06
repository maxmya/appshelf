# Build a local package from this checkout: makepkg -f
pkgname=appshelf
pkgver=0.4.0
pkgrel=1
pkgdesc='Omarchy-first AppImage manager with a Quickshell UI'
arch=('x86_64' 'aarch64')
license=('MIT')
depends=('quickshell' 'omarchy' 'desktop-file-utils' 'xdg-utils')
makedepends=('cargo' 'curl')
optdepends=('flea: reveal managed applications')

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
  cp -r "$startdir/ui" "$startdir/vendor" "$pkgdir/usr/share/appshelf/"
  install -m755 "$startdir/target/release/appshelf" "$pkgdir/usr/share/appshelf/appshelf"
  ln -s /usr/share/appshelf/appshelf "$pkgdir/usr/bin/appshelf"
  ln -sfn /usr/share/omarchy/shell/Commons "$pkgdir/usr/share/appshelf/ui/Commons"
  install -Dm644 "$startdir/packaging/org.omarchy.appshelf.desktop" "$pkgdir/usr/share/applications/org.omarchy.appshelf.desktop"
  install -Dm644 "$startdir/packaging/org.omarchy.appshelf.svg" "$pkgdir/usr/share/icons/hicolor/scalable/apps/org.omarchy.appshelf.svg"
  install -Dm644 "$startdir/packaging/org.omarchy.appshelf.png" "$pkgdir/usr/share/icons/hicolor/512x512/apps/org.omarchy.appshelf.png"
  install -Dm644 "$startdir/LICENSE" "$pkgdir/usr/share/licenses/appshelf/LICENSE"
}
