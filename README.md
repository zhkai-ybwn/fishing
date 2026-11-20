# BossSensor

BossSensor 是一个本地运行的 Windows 摄像头工具。可以把实时画面放在置顶小窗里，也可以在识别到指定人员或第二张脸时提示、切换到选好的应用窗口。

## 安装和使用

1. 从 [GitHub Releases](https://github.com/zhkai-ybwn/fishing/releases) 下载 Windows x64 的 `Setup.exe` 或 `.msi`，安装后打开 BossSensor。
2. 选择摄像头。按需打开置顶预览、触发条件和触发动作，然后点击「启动」。只打开预览即可查看画面。
3. 要识别指定人员，输入名称并点击「录入」。让同意录入的人单独正对摄像头，直到设置页显示录入成功。
4. 要自动切屏，先打开工作应用，再从目标列表选应用或具体窗口。如果同一应用打开了多个窗口，选择具体窗口可消除歧义。

「第二张脸」以同一帧可见的人脸数为准。让自己的脸留在画面中，便于区分是否有人进入。指定人员匹配和人数判断的效果受距离、光线和遮挡影响。每条规则在稳定命中后触发一次，离开后重新就绪。

Windows 可能拒绝后台程序激活其他窗口。发生时，设置页显示切屏失败，已开启的桌面提示仍会显示。摄像头帧只用于当前预览和推理。配置和录入的人脸特征保存在 `%LOCALAPPDATA%\BossSensor\settings.json`，可在设置页移除已录入人员。

## 从源码构建

开发环境需要 Windows x64、PowerShell 7、Rust 1.90.0 和 Visual Studio 2022 的 C++ x64 构建工具。运行：

```powershell
./packaging/Prepare-Assets.ps1
$runtime = & ./packaging/Get-OnnxRuntime.ps1
$env:ORT_DYLIB_PATH = Join-Path $runtime 'onnxruntime.dll'
cargo run --locked --bin BossSensor
```

脚本下载固定版本的模型、字体和 ONNX Runtime，并核对 SHA-256。发布构建把模型与字体嵌入程序，安装包同时带上运行库，安装后即可离线使用。

维护、验证和发布步骤见 [CONTRIBUTING.md](CONTRIBUTING.md)。项目许可见 [LICENSE](LICENSE)，模型和运行库许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
