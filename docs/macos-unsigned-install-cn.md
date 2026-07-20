# macOS 未签名 DMG 安装说明

本文适用于从 GitHub Releases 下载的、未经过 Apple Developer ID 签名和公证的
`Agent Activity Hub` macOS 安装包。红绿灯是应用内浮窗，安装后应从“应用程序”中启动
`Agent Activity Hub`，不会出现名为“红绿灯”的独立应用。

## 适用范围

当前 `Agent.Activity.Hub_0.1.0_aarch64.dmg` 是 Apple Silicon 版本，只能运行在
`arm64` Mac 上。目标电脑可以先执行：

```bash
uname -m
```

输出为 `arm64` 才能使用这个安装包。输出为 `x86_64` 的 Intel Mac 需要单独构建
Intel 版本。

## 下载和校验

从仓库的 [Releases](https://github.com/VICIy/agent-activity-hub/releases)
页面下载 DMG。当前版本的 SHA-256 为：

```text
1c84d09244ed4287d5a9081b6c74ec8301d9d5870726f5eb0e7df2de8698740a
```

在下载目录执行校验：

```bash
shasum -a 256 "$HOME/Downloads/Agent.Activity.Hub_0.1.0_aarch64.dmg"
```

只有校验值一致时才继续安装。

## 安装

1. 双击 DMG。
2. 将 `Agent Activity Hub.app` 拖到“应用程序”目录。
3. 在 Finder 中右键应用，选择“打开”。
4. 如果系统提示应用来源未知，进入“系统设置 → 隐私与安全性”，选择“仍要打开”。

## 仍然显示“文件已损坏”

确认 DMG 来源可信且 SHA-256 校验一致后，在目标电脑执行以下命令。命令会清理
下载隔离标记，并在本机重新生成 ad-hoc 签名：

```bash
sudo xattr -cr "/Applications/Agent Activity Hub.app"
sudo codesign --force --deep --sign - "/Applications/Agent Activity Hub.app"
open "/Applications/Agent Activity Hub.app"
```

如果应用安装在用户目录，将路径替换为实际路径，并去掉不需要的 `sudo`：

```bash
xattr -cr "$HOME/Applications/Agent Activity Hub.app"
codesign --force --deep --sign - "$HOME/Applications/Agent Activity Hub.app"
open "$HOME/Applications/Agent Activity Hub.app"
```

不要使用 `spctl --master-disable` 全局关闭 Gatekeeper，也不要对来源不明的应用
执行上述命令。

## 首次启动后配置 Agent Hook

应用启动后，在 Tauri 控制面板打开“适配器”，点击“检测”，再安装或修复正在使用
的 Codex、Claude Code、Qoder Hook。应用使用内置本地 Hook Helper，不需要启动旧的
`8765` 或 `8766` HTTP 服务。

## 正式发布

未签名安装包适合个人设备或明确受信任的测试设备。要让其他用户直接双击安装而不看
到 Gatekeeper 警告，需要 Apple Developer Program 的 `Developer ID Application`
证书、签名和 Apple 公证。没有这些凭据时，重复构建 DMG 不会自动获得信任。
