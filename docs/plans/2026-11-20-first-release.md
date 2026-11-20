# fishing first release implementation plan

**Goal.** Ship an offline Windows x64 client with an optional live preview, configurable local detection, and EXE and MSI installers.

**Architecture.** An eframe application owns settings and a rule state machine. A worker owns capture and inference. The preview consumes only the newest frame, and the application executes desktop actions when the rule state changes. Keep platform and model details outside the rules.

1. Build the Windows adapter for cameras and selectable desktop windows. Verify enumeration, invalid-device errors, and foreground activation results.
2. Build CPU inference with pinned YuNet and SFace weights. Verify both ONNX graphs and the feature matching pipeline. Package the model and runtime licenses.
3. Implement settings, local templates, a draggable preview, and independent rule and action controls. Test entering, leaving, simultaneous matches, and camera interruptions without a camera.
4. Build both installers from the same release executable and runtime files. Add CI for formatting, linting, tests, and versioned GitHub release artifacts.
5. Verify the release build and installer contents. Check installation, upgrade, and uninstall where an isolated Windows installation is available.
