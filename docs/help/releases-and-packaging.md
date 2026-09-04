# Releases and macOS packaging

Use this guide to synchronize workspace versioning and package the macOS runtime applications.

## Versioning

Runinator uses one `major.minor.build` version across the Cargo workspace, command center, VS Code
extension, packaged backend apps, release archives, and container images. Major and minor are
release decisions and are set in the root `Cargo.toml`; the build is the full-history Git commit
count. Release CI runs `node scripts/set-workspace-version.mjs` after a full checkout to resolve the
build number and synchronize the non-Cargo manifests. Container builds receive the same version as
an OCI label, and the default Kubernetes tag is `<version>-kube-<timestamp>`.

To prepare a manual major or minor bump, edit the root workspace version and run:

```bash
node scripts/set-workspace-version.mjs
cargo metadata --no-deps --format-version 1 >/dev/null
```


## Package macOS Runtime Apps

The Rust services and desktop agent remain normal binaries. On macOS, you can
also package them as `.app` bundles with the Runinator icon:

```bash
cargo install cargo-packager --version 0.11.8 --locked
scripts/package-macos-backend-apps.sh --release
```

The script creates `.app` bundles for broker, web service, waker, headless
worker, desktop agent, the control CLI (`runinatorctl`), and supervisor under
`target/macos-apps`.

The macOS release archive places **Runinator Desktop Agent.app** at its top
level. Launch that bundle rather than its executable under `Contents/MacOS`:
macOS reads the bundle's `Info.plist` at launch to show **Runinator Desktop
Agent** in the Dock.
