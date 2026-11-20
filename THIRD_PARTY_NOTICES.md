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

The bundled font comes from [Google Fonts at commit `a85815a4`](https://github.com/google/fonts/tree/a85815a42757630ce188fdad368c2dfc444d4773/ofl/notosanssc). Its SHA-256 is `a3041811a78c361b1de50f953c805e0244951c21c5bd412f7232ef0d899af0da`.

The offline inference self-test uses a crop of the U.S. Navy portrait of Grace M. Hopper by James S. Davis, a U.S. public-domain photograph. [Fixture source](crates/vision/tests/data/README.md)
