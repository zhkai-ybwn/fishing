# Third-party notices

fishing source code is MIT-licensed. Bundled model weights and runtime libraries retain their own licenses.

| Component | License and source |
| --- | --- |
| YuNet model | MIT. [Notice](crates/vision/models/YuNet-LICENSE) |
| SFace model | Apache 2.0. [Notice](crates/vision/models/SFace-LICENSE) |
| Noto Sans SC | SIL Open Font License 1.1. [Notice](assets/fonts/OFL.txt) |
| ONNX Runtime | MIT. [ONNX Runtime](https://github.com/microsoft/onnxruntime). The runtime's license and third-party notices are included in the installers. |
| Microsoft Visual C++ runtime | Redistributable files from the x64 Visual Studio build tools. [Redistribution documentation](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files) |

The model sources and hashes are recorded in [the model manifest](crates/vision/models/README.md). The source dependencies are tracked in `Cargo.lock`.

The bundled Noto Sans SC Regular and Bold files come from [Noto CJK Sans 2.004 at commit `523d033`](https://github.com/notofonts/noto-cjk/tree/523d033d6cb47f4a80c58a35753646f5c3608a78/Sans/SubsetOTF/SC). Both SHA-256 values are pinned in [the asset preparation script](packaging/Prepare-Assets.ps1).

The offline inference self-test uses a crop of the U.S. Navy portrait of Grace M. Hopper by James S. Davis, a U.S. public-domain photograph. [Fixture source](crates/vision/tests/data/README.md)
