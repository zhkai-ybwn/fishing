# Contributing to fishing

fishing targets Windows x64. Read the build instructions in [README.md](README.md) to prepare the development environment.

## Code boundaries

- `src/app.rs` owns the settings window and floating preview. Keep the main window's native Windows title bar, expose secondary settings when relevant, and show actionable camera and window errors. Review light and dark themes and keyboard navigation using the macOS design principles agreed for this project.
- `src/engine.rs` owns the capture and inference workers. Workers share the newest frame only. Desktop actions run off the UI thread and a change of camera or policy invalidates old readings.
- `src/rules.rs` converts observations into debounced presence events. Use deterministic tests for entry, exit, and overlapping matches.
- `src/settings.rs` owns the local schema and persists enrolled features with their model identifier. A newer schema must preserve the settings produced by a released version or report that it cannot load them.
- `crates/platform` isolates Windows camera, window and notification APIs. `crates/vision` owns preprocessing, postprocessing, pinned models and feature extraction.
- `crates/update` checks GitHub Releases and verifies staged installers. `src/update_ui.rs` owns the user-initiated check, download, and launch flow.

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
- At the default main-window dimensions, enable every home toggle and confirm that the complete two-column panel fits. Select a long window title and check that the Refresh button remains visible. Resize to check the compact single-column layout and open Settings to reach the secondary controls. In the frameless preview, test drag, resize, brightness, and the right-click close command.
- Check the default threshold of two visible faces, then type three into the field. Check the I-beam cursor and commit the value with Enter and by clicking away. Verify the rule starts at the chosen count and includes the camera operator whenever they appear in frame.
- Check the Chinese labels and headings in light and dark themes. Confirm buttons and switches have immediate visible feedback on pointer-down.
- Use 「预览提醒」 with fixed custom text and with a `{时长}` template. Confirm the default can be restored, previously saved plain text is displayed literally, and elapsed time restarts when detection is started from the main window or tray.
- Use 「收起到托盘」 while detection is running. Check that the taskbar button and preview disappear and that configured actions keep working. Open settings from the tray, try starting and pausing detection there, and exit via the tray menu.
- Check that the main window prompts by default, including when loading a schema-one settings file saved with `exit`. Try cancel, exit and tray. Remember a choice and confirm it remains selected on relaunch. Watch the last visible frame when closing or minimizing in both Windows themes.
- Check a foreground switch with the target application available. When Windows restricts activation, verify that the status distinguishes a target brought forward visually from a target with keyboard focus. When visual raising succeeds, confirm that the original application retains keyboard input. When both attempts fail, check that the status reports the failure.

## Building the installers

Install WiX 6.0.2 and its matching Bootstrapper Applications and UI extensions. Use the package version from `Cargo.toml` for both installers.

```powershell
./packaging/Prepare-Assets.ps1
./packaging/Get-OnnxRuntime.ps1
wix extension add -g WixToolset.BootstrapperApplications.wixext/6.0.2
wix extension add -g WixToolset.UI.wixext/6.0.2
cargo build --release --locked --bin fishing
./packaging/Build-Installers.ps1 -Version 0.2.0
```

The output contains a per-user MSI, its self-contained EXE installer, and `SHA256SUMS.txt`. Verify the client locally before publishing. Check that both installers create the Start menu shortcut and honor the desktop shortcut selection. Check shortcut removal on uninstall and settings retention on upgrade. Keep the package upgrade identity stable. When a newer stable release is available, test the update flow from an older update-capable client through the interactive installer.

After local acceptance, tag the verified commit `v<version>`. Tag pushes build and smoke-test the installers and leave them as Actions artifacts. To publish, manually run the Windows workflow on that tag with `publish` enabled. The workflow validates the tag, runs the installer smoke tests and publishes both files with their checksums.
