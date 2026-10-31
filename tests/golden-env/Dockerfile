# Canonical environment for the shell golden screenshots (tests/golden.rs).
# Frozen to the Arch Linux Archive as of 2026-10-30, the day the host that made the goldens died:
# GTK 4.24.1, libadwaita 1.10.0, cairo 1.18.6, pango 1.58.2, Xvfb 21.1.25. Fonts matter as much:
# kit.css asks for Inter first, and the goldens were made without it, so the UI renders in
# Cantarell and keycaps in DejaVu Sans. Do not add Inter here. gimp is only there for its app\n# icon (the flatpak-consent goldens show GIMP). Run it with ./run.sh.
FROM archlinux:base
ARG SNAPSHOT=2026/10/30
RUN printf 'Server = https://archive.archlinux.org/repos/%s/$repo/os/$arch\n' "$SNAPSHOT" > /etc/pacman.d/mirrorlist \
 && pacman -Syyuu --noconfirm --disable-sandbox \
 && pacman -S --noconfirm --needed --disable-sandbox \
    base-devel git rust \
    gtk4 libadwaita adwaita-icon-theme xorg-server-xvfb \
    adwaita-fonts cantarell-fonts ttf-dejavu noto-fonts noto-fonts-emoji otf-font-awesome ttf-jetbrains-mono \
 && pacman -Scc --noconfirm \
 && pacman -Q gtk4 libadwaita cairo pango harfbuzz fontconfig xorg-server-xvfb > /etc/golden-versions
ENV HOME=/tmp/home CARGO_HOME=/target/cargo-home XDG_RUNTIME_DIR=/tmp/xdg
WORKDIR /work/crates/shell


