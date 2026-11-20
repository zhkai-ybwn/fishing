<div align="center">
  <img src="assets/fishing.png" width="116" height="116" alt="fishing 小鱼标志" />
  <h1>fishing</h1>
  <p>Windows 桌面摸鱼助手</p>
</div>

把摄像头画面放在桌面角落，按画面中的人数或指定人员触发提醒、切换窗口。预览、识别条件和触发动作可以独立配置。

## 下载

首版支持 Windows x64。发布后可下载 EXE 或 MSI 安装包，任选一种安装。 [GitHub Releases](https://github.com/zhkai-ybwn/fishing/releases)

## 怎么用

选择摄像头，按需配置下列功能，然后点「启动」。

- **悬浮预览。** 打开「置顶实时预览」即可把画面放在桌面小窗里，也可以只使用这个功能。
- **按人数触发。** 打开「按人脸数量触发」，输入正整数并按 Enter。默认 2 张，自己出现在画面中时也计入。
- **识别指定人员。** 打开「识别指定人员」，填写名称后点「从照片录入…」。使用仅有一张清晰正脸的 JPG 或 PNG，也可以用摄像头录入。
- **选择触发动作。** 「显示桌面提示」和「切到目标应用」可以同时开启。切窗目标从已打开的应用或窗口中选择。

Windows 限制后台应用激活目标窗口时，fishing 会尝试把目标窗口置前，键盘焦点由原窗口持有。实际切窗结果显示在主窗口。

### 提醒与托盘

「提示文字」可以使用固定文案，也可以用 `{时长}` 显示从主窗口或托盘启动本次检测以来的时间。例如输入 `已专注{时长}，休息一下吧。`。用「预览提醒」查看效果，用「恢复默认」换回默认文案。

「收起到托盘」会隐藏设置、任务栏按钮和悬浮预览。运行中的检测和触发动作会继续执行。右键托盘图标可以重新打开设置、启动或暂停检测，也可以退出 fishing。

### 本地处理

摄像头画面和导入的照片都在本机处理。录入时保存人员名称和比对特征，可随时在设置中移除。

## 从源码运行

使用 Windows x64、PowerShell 7、Rust 1.90.0 和 Visual Studio 2022 C++ 构建工具。

```powershell
./packaging/Prepare-Assets.ps1
./packaging/Get-OnnxRuntime.ps1
cargo run --locked --bin fishing
```

前两步会准备并校验固定版本的模型、字体和 ONNX Runtime。构建时会把运行库放到应用同目录。

[贡献和发布流程](CONTRIBUTING.md) · [MIT 许可证](LICENSE) · [模型及依赖许可](THIRD_PARTY_NOTICES.md)
