#!/usr/bin/env bash

set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
release_tag="${1:-${RELEASE_TAG:-}}"

if [[ ! "$release_tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-[0-9A-Za-z][0-9A-Za-z.-]*$ ]]; then
  echo "release tag must be a SemVer prerelease such as v0.1.0-mvp.1" >&2
  exit 1
fi

release_version="${release_tag#v}"
base_version="${release_version%%-*}"
prerelease="${release_version#*-}"

if [[ "$prerelease" == *..* || "$prerelease" == *. ]]; then
  echo "prerelease identifiers must be non-empty" >&2
  exit 1
fi
IFS='.' read -r -a prerelease_identifiers <<< "$prerelease"
for identifier in "${prerelease_identifiers[@]}"; do
  if [[ "$identifier" =~ ^[0-9]+$ && "$identifier" != "0" && "$identifier" == 0* ]]; then
    echo "numeric prerelease identifiers must not contain leading zeroes: $identifier" >&2
    exit 1
  fi
done

tauri_version="$(sed -nE 's/^[[:space:]]*"version":[[:space:]]*"([^"]+)".*/\1/p' "$repository_root/desktop/src-tauri/tauri.conf.json" | head -n 1)"
frontend_version="$(sed -nE 's/^[[:space:]]*"version":[[:space:]]*"([^"]+)".*/\1/p' "$repository_root/desktop/package.json" | head -n 1)"
cargo_version="$(awk '
  /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
  /^\[/ { in_workspace_package = 0 }
  in_workspace_package && /^version[[:space:]]*=/ {
    value = $0
    sub(/^[^=]*=[[:space:]]*"/, "", value)
    sub(/".*/, "", value)
    print value
    exit
  }
' "$repository_root/desktop/src-tauri/Cargo.toml")"
android_version="$(sed -nE 's/^[[:space:]]*versionName[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' "$repository_root/android/app/build.gradle.kts" | head -n 1)"

declare -A versions=(
  [tauri]="$tauri_version"
  [frontend]="$frontend_version"
  [cargo]="$cargo_version"
  [android]="$android_version"
)

for component in tauri frontend cargo android; do
  if [[ -z "${versions[$component]}" ]]; then
    echo "could not read $component version metadata" >&2
    exit 1
  fi
  if [[ "${versions[$component]}" != "$base_version" ]]; then
    echo "$component version ${versions[$component]} does not match tag base version $base_version" >&2
    exit 1
  fi
done

echo "release tag $release_tag matches base version $base_version across all components"

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    echo "release_tag=$release_tag"
    echo "release_version=$release_version"
    echo "base_version=$base_version"
  } >> "$GITHUB_OUTPUT"
fi
