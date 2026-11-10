#!/usr/bin/env bash
# Builds Crowbar_Qt-<version>-x86_64.AppImage with linuxdeploy and its Qt plugin.
# The AppImage runs on distributions whose glibc is at least as new as the
# build machine's, so release builds belong on an older base (e.g. Ubuntu 22.04).
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
work=${WORK:-$root/build-appimage}
tools=$work/tools
mkdir -p "$tools"

fetch() {
    [ -x "$tools/$1" ] && return
    curl -fL --retry 3 -o "$tools/$1" "$2"
    chmod +x "$tools/$1"
}
fetch linuxdeploy-x86_64.AppImage \
    https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
fetch linuxdeploy-plugin-qt-x86_64.AppImage \
    https://github.com/linuxdeploy/linuxdeploy-plugin-qt/releases/download/continuous/linuxdeploy-plugin-qt-x86_64.AppImage

cmake -B "$work/build" -S "$root" -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr -DBUILD_TESTING=OFF
cmake --build "$work/build" -j"$(nproc)"
rm -rf "$work/AppDir"
DESTDIR="$work/AppDir" cmake --install "$work/build"

export QMAKE=${QMAKE:-$(command -v qmake6 || command -v qmake)}
# Without the Wayland plugins the AppImage only runs under X11/XWayland; not every Qt build ships them.
platforms=$("$QMAKE" -query QT_INSTALL_PLUGINS)/platforms
extra=()
for plugin in libqwayland-egl.so libqwayland-generic.so; do
    [ -f "$platforms/$plugin" ] && extra+=("$plugin")
done
if [ ${#extra[@]} -gt 0 ]; then
    EXTRA_PLATFORM_PLUGINS=$(IFS=';'; echo "${extra[*]}")
    export EXTRA_PLATFORM_PLUGINS
else
    echo "warning: no Qt Wayland plugins in $platforms; the AppImage will use XWayland" >&2
fi
version=$(sed -n 's/^project(crowbar-qt VERSION \([0-9.]*\).*/\1/p' "$root/CMakeLists.txt")
export OUTPUT=${OUTPUT:-Crowbar_Qt-$version-x86_64.AppImage}

cd "$work"
# An AppImage must carry an icon, so a project without one gets a blank placeholder.
icon=$(find AppDir/usr/share/icons AppDir/usr/share/pixmaps -name 'crowbar-qt.*' 2>/dev/null | head -n1)
if [ -z "$icon" ]; then
    icon=$work/crowbar-qt.svg
    printf '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"/>\n' > "$icon"
fi
PATH="$tools:$PATH" "$tools/linuxdeploy-x86_64.AppImage" \
    --appdir AppDir \
    --desktop-file AppDir/usr/share/applications/crowbar-qt.desktop \
    --icon-file "$icon" \
    --plugin qt \
    --output appimage
echo "Built $work/$OUTPUT"
