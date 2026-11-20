# Contributing to fishing

fishing targets Windows x64. Read the build instructions in [README.md](README.md) to prepare the development environment.

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

For changes affecting the desktop experience, use the local application for manual verification.

- Try preview-only and person enrollment. Cover a disconnected or unavailable camera and the transition back to a working device. Test the office camera angle, distance, and lighting separately from the model fixtures.
- Check the default threshold of two visible faces, then change it to three. Verify the rule starts at the chosen count and includes the camera operator whenever they appear in frame.
- Minimize the settings window and verify the default popup text. Enter custom text and verify the next popup, then clear the field and check that the default is used.
- Check a foreground switch with the target application available. When Windows restricts activation, verify that the status distinguishes a target brought forward visually from a target with keyboard focus. When visual raising succeeds, confirm that the original application retains keyboard input. When both attempts fail, check that the status reports the failure.

## Building the installers

Install WiX 6.0.2 with the matching `WixToolset.BootstrapperApplications.wixext` extension. The package version in `Cargo.toml` supplies the version for both installers. Build locally with:

```powershell
./packaging/Prepare-Assets.ps1
./packaging/Get-OnnxRuntime.ps1
wix extension add -g WixToolset.BootstrapperApplications.wixext/6.0.2
cargo build --release --locked --bin fishing
./packaging/Build-Installers.ps1 -Version 0.1.0
```

The output contains a per-user MSI, its self-contained EXE installer, and `SHA256SUMS.txt`. Verify the client locally before publishing. Check the real camera preview, rule and action combinations, settings in both system themes, and installation and removal. For upgrades, check that settings from the last published version remain intact. Keep the package upgrade identity stable.

After local acceptance, tag the verified commit `v<version>`. Tag pushes build and smoke-test the installers and leave them as Actions artifacts. To publish, manually run the Windows workflow on that tag with `publish` enabled. The workflow validates the tag, runs the installer smoke tests and publishes both files with their checksums.
