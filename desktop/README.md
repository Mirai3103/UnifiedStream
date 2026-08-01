# UnifiedStream desktop development

This directory contains the React, TypeScript, and Tauri 2 Linux desktop application. User installation and operating instructions live in the repository's [main README](../README.md) and [Linux guide](../docs/linux-installation.md).

## Prerequisites

- Bun 1.3.14
- Rust 1.82 or newer
- Tauri 2 Linux build dependencies
- PipeWire development headers

Ubuntu package names used by CI are listed in [the quality workflow](../.github/workflows/quality.yml).

## Develop

```bash
bun install --frozen-lockfile
bun run tauri dev
```

## Validate

```bash
bun install --frozen-lockfile
bun run build

cd src-tauri
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins -- -D warnings
cargo test --workspace
```

## Build Linux bundles

From the repository root, use the release script described in [the release guide](../docs/releasing.md). Direct Tauri builds can be run from this directory:

```bash
bun install --frozen-lockfile
bun run tauri build --bundles appimage,deb --ci
```
