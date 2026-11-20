# Contributing to BossSensor

BossSensor targets Windows x64. Read the build instructions in [README.md](README.md) to prepare the development environment.

## Code boundaries

- `src/app.rs` owns the settings window and floating preview. Keep the native Windows title bar, expose secondary settings when relevant, and show actionable camera and window errors. Review light and dark themes and keyboard navigation using the macOS design principles agreed for this project.
- `src/engine.rs` owns the capture and inference workers. Workers share the newest frame only. Desktop actions run off the UI thread and a change of camera or policy invalidates old readings.
- `src/rules.rs` converts observations into debounced presence events. Use deterministic tests for entry, exit, and overlapping matches.
- `src/settings.rs` owns the local schema and persists enrolled features with their model identifier. A newer schema must preserve the settings produced by a released version or report that it cannot load them.
- `crates/platform` isolates Windows camera, window and notification APIs. `crates/vision` owns preprocessing, postprocessing, pinned models and feature extraction.

## Checks

```powershell
./packaging/Prepare-Assets.ps1
$runtime = & ./packaging/Get-OnnxRuntime.ps1
$env:ORT_DYLIB_PATH = Join-Path $runtime 'onnxruntime.dll'
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

For changes affecting the desktop experience, manually verify preview-only, notifications with the main window minimized, separate detection switches, person enrollment, and a failed foreground switch. Cover a disconnected or unavailable camera and the transition back to a working device. The office camera angle, distance and lighting are worth testing separately from the model fixtures.

## Building the installers

Install WiX 6.0.2 with the matching `WixToolset.BootstrapperApplications.wixext` extension. The package version in `Cargo.toml` supplies the version for both installers. Build locally with:

```powershell
./packaging/Prepare-Assets.ps1
wix extension add -g WixToolset.BootstrapperApplications.wixext/6.0.2
cargo build --release --locked --bin BossSensor
./packaging/Build-Installers.ps1 -Version 0.1.0
```

The output contains a per-user MSI, its self-contained EXE installer, and `SHA256SUMS.txt`. Verify the client locally before publishing. Check the real camera preview, rule and action combinations, settings in both system themes, and installation and removal. For upgrades, check that settings from the last published version remain intact. Keep the package upgrade identity stable.

After local acceptance, tag the verified commit `v<version>`. Tag pushes build and smoke-test the installers and leave them as Actions artifacts. To publish, manually run the Windows workflow on that tag with `publish` enabled. The workflow validates the tag, runs the installer smoke tests and publishes both files with their checksums.
