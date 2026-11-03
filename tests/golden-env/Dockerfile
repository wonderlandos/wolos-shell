# Canonical environment for the shell golden screenshots (tests/golden.rs).
# Frozen to the Arch Linux Archive as of 2026-10-30, the day the host that made the goldens died:
# GTK 4.24.1, libadwaita 1.10.0, cairo 1.18.6, pango 1.58.2, Xvfb 21.1.25. Fonts matter as much:
# kit.css asks for Inter first (the shipped UI typeface, wolos-iso/profile/packages.desktop), so the image
# carries inter-font and the goldens render in the type the product ships. Cantarell stays as the
# fallback the stylesheet names, DejaVu Sans for keycaps. tests/golden.rs asserts that Pango resolves
# the UI family to Inter (golden_cases/font.rs): a golden made without it would lock the wrong type.
# gimp is only there for its app icon (the flatpak-consent goldens show GIMP). Run it with ./run.sh.
FROM archlinux:base@sha256:e82623936fa1a69700b48e98b058df6eaab18b737c27473a7234a40fdecb9413
ARG SNAPSHOT=2026/10/30
RUN printf 'Server = https://archive.archlinux.org/repos/%s/$repo/os/$arch\n' "$SNAPSHOT" > /etc/pacman.d/mirrorlist \
 && pacman -Syyuu --noconfirm --disable-sandbox \
 && pacman -S --noconfirm --needed --disable-sandbox \
    base-devel git rust \
    gtk4 libadwaita adwaita-icon-theme xorg-server-xvfb \
    adwaita-fonts inter-font cantarell-fonts ttf-dejavu noto-fonts noto-fonts-emoji otf-font-awesome ttf-jetbrains-mono \
 && pacman -Scc --noconfirm \
 && pacman -Q gtk4 libadwaita cairo pango harfbuzz fontconfig xorg-server-xvfb inter-font > /etc/golden-versions
ENV HOME=/tmp/home CARGO_HOME=/target/cargo-home XDG_RUNTIME_DIR=/tmp/xdg
WORKDIR /work/crates/shell


