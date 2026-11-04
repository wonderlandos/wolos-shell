# Canonical environment for the shell golden screenshots (tests/golden.rs).
# Frozen to the Arch Linux Archive as of 2026-10-30, the day the host that made the goldens died:
# GTK 4.24.1, libadwaita 1.10.0, cairo 1.18.6, pango 1.58.2, Xvfb 21.1.25. Fonts matter as much:
# kit.css asks for Inter first (the shipped UI typeface, wolos-iso/profile/packages.desktop), so the image
# carries inter-font and the goldens render in the type the product ships. Cantarell stays as the
# fallback the stylesheet names, DejaVu Sans for keycaps. tests/golden.rs asserts that Pango resolves
# the UI family to Inter (golden_cases/font.rs): a golden made without it would lock the wrong type;
# the build stops here already when fontconfig does not answer Inter for Inter.
# noto-fonts-cjk: the ISO ships it (wolos-iso profile/packages.desktop), and the Japanese Setup and installer
# goldens (L10N-2) need a face for their glyphs; Inter and the Latin fallbacks are asked first, so no Latin golden moves.
# Icons come from the repo's design/ tree only (kit/style.rs lays it out as the WolOS theme;
# golden.rs host_icons fails a case that draws one from an installed theme), so no app package is
# installed for its icon: a third-party app in a golden shows the package fallback.
# clang and libpipewire: crates/audio builds its pipewire binding by default (bindgen needs both).
# Run ./run.sh.
FROM archlinux:base@sha256:e82623936fa1a69700b48e98b058df6eaab18b737c27473a7234a40fdecb9413
ARG SNAPSHOT=2026/10/30
RUN printf 'Server = https://archive.archlinux.org/repos/%s/$repo/os/$arch\n' "$SNAPSHOT" > /etc/pacman.d/mirrorlist \
 && pacman -Syyuu --noconfirm --disable-sandbox \
 && pacman -S --noconfirm --needed --disable-sandbox \
    base-devel git rust clang libpipewire \
    gtk4 libadwaita adwaita-icon-theme xorg-server-xvfb \
    adwaita-fonts inter-font cantarell-fonts ttf-dejavu noto-fonts noto-fonts-cjk noto-fonts-emoji otf-font-awesome ttf-jetbrains-mono \
 && pacman -Scc --noconfirm \
 && pacman -Q gtk4 libadwaita cairo pango harfbuzz fontconfig xorg-server-xvfb inter-font > /etc/golden-versions \
 && test "$(fc-match -f '%{family[0]}' Inter)" = Inter
ENV HOME=/tmp/home CARGO_HOME=/target/cargo-home XDG_RUNTIME_DIR=/tmp/xdg
WORKDIR /work/crates/shell


