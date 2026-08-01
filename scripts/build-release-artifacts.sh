#!/usr/bin/env bash

set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
release_tag="${1:-${RELEASE_TAG:-}}"
output_input="${2:-release-dist}"

"$repository_root/scripts/check-release-version.sh" "$release_tag"

release_version="${release_tag#v}"
case "$(uname -m)" in
  x86_64 | amd64) architecture="x86_64" ;;
  aarch64 | arm64) architecture="aarch64" ;;
  *)
    echo "unsupported release architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

if [[ "$output_input" = /* ]]; then
  output_dir="$output_input"
else
  output_dir="$repository_root/$output_input"
fi
mkdir -p "$output_dir"

desktop_name="UnifiedStream-${release_version}-linux-${architecture}"
android_name="UnifiedStream-${release_version}-android-universal-debug.apk"
destinations=(
  "$output_dir/${desktop_name}.AppImage"
  "$output_dir/${desktop_name}.deb"
  "$output_dir/$android_name"
  "$output_dir/SHA256SUMS"
)
for destination in "${destinations[@]}"; do
  if [[ -e "$destination" ]]; then
    echo "refusing to overwrite existing release output: $destination" >&2
    exit 1
  fi
done

(
  cd "$repository_root/desktop"
  bun install --frozen-lockfile
  # linuxdeploy bundles distribution libraries. Its embedded strip can be older than the host
  # ELF format (for example RELR on current Arch), so preserve symbols instead of corrupting or
  # rejecting otherwise valid libraries.
  NO_STRIP=1 bun run tauri build --bundles appimage,deb --ci
)

(
  cd "$repository_root/android"
  ./gradlew testDebugUnitTest assembleDebug --no-daemon
)

mapfile -t appimages < <(find "$repository_root/desktop/src-tauri/target/release/bundle/appimage" -maxdepth 1 -type f -name '*.AppImage' -print)
mapfile -t debs < <(find "$repository_root/desktop/src-tauri/target/release/bundle/deb" -maxdepth 1 -type f -name '*.deb' -print)
mapfile -t apks < <(find "$repository_root/android/app/build/outputs/apk/debug" -maxdepth 1 -type f -name '*-debug.apk' -print)

if [[ "${#appimages[@]}" -ne 1 || "${#debs[@]}" -ne 1 || "${#apks[@]}" -ne 1 ]]; then
  echo "expected exactly one AppImage, one Debian package, and one debug APK" >&2
  printf 'AppImages: %s\nDebian packages: %s\nDebug APKs: %s\n' "${#appimages[@]}" "${#debs[@]}" "${#apks[@]}" >&2
  exit 1
fi

install -m 0755 "${appimages[0]}" "$output_dir/${desktop_name}.AppImage"
install -m 0644 "${debs[0]}" "$output_dir/${desktop_name}.deb"
install -m 0644 "${apks[0]}" "$output_dir/$android_name"

(
  cd "$output_dir"
  sha256sum "${desktop_name}.AppImage" "${desktop_name}.deb" "$android_name" > SHA256SUMS
  sha256sum --check SHA256SUMS
)

echo "release artifacts are ready in $output_dir"
