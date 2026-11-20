<div align="center">
  <img src="assets/fishing.png" width="116" height="116" alt="fishing 小鱼标志" />
  <h1>fishing</h1>
  <p>Windows 桌面摸鱼助手</p>
</div>

把摄像头画面放在桌面角落，有人走近时可以提醒你，也可以切到预选的工作窗口。悬浮预览、识别条件和触发动作都能单独设置。

## 怎么用

| 场景 | 设置 |
| --- | --- |
| 只想随时瞄一眼 | 打开「置顶实时预览」 |
| 想知道有同事走近 | 打开「画面中出现第二张脸」，让自己留在画面里 |
| 想在某人出现时收到提醒 | 启用「识别指定人员」，录入照片，再打开「显示桌面提示」 |
| 想自动切到工作窗口 | 打开「切到目标应用」，从已打开的应用或窗口里选目标 |

配好后点击「启动」。Windows 有时会限制后台程序切换前台窗口。发生时，主窗口会显示切换结果，已开启的提示仍会发出。

### 从照片录入

在「触发条件」打开「识别指定人员」，填一个名称，点「从照片录入…」。选一张你有权使用的 JPG 或 PNG，照片里要有一张清晰正脸。对方在场时，也可以点「摄像头录入」。

画面和导入的照片都在本机处理。保存的是用来比对的特征和名称，随时可以在设置里移除。检测效果受拍摄距离、光线和遮挡影响。

## 安装

首版支持 Windows x64。验收完成后会提供 EXE 和 MSI 两种安装包。使用免安装版时，从完整目录运行 `fishing.exe`，同目录 DLL 一起保留。 [GitHub Releases](https://github.com/zhkai-ybwn/fishing/releases)

## 从源码运行

使用 Windows x64、PowerShell 7、Rust 1.90.0 和 Visual Studio 2022 C++ 构建工具。

```powershell
./packaging/Prepare-Assets.ps1
./packaging/Get-OnnxRuntime.ps1
cargo run --locked --bin fishing
```

前两步下载并校验固定版本的模型、字体和运行库。构建会把 ONNX Runtime DLL 放到程序同目录。如果旧构建报 `ONNX Runtime library not found`，按上面的顺序准备依赖并重新构建，再运行新的 `fishing.exe`。

[贡献和发布流程](CONTRIBUTING.md) · [MIT 许可证](LICENSE) · [模型及依赖许可](THIRD_PARTY_NOTICES.md)