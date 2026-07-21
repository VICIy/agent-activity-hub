# Agent Activity Hub

[English](README.md) | [简体中文](README-cn.md)

Agent Activity Hub 是一款本地优先的 Tauri 桌面应用，用统一的会话状态机汇总
Codex、Claude Code、Qoder 和自定义 Agent 的活动，并通过红绿灯浮窗展示当前
最需要关注的状态。浮窗是主要输出，即使没有外接 LED 设备也可以完整使用。

```text
Agent Hook 与会话日志
  -> 应用内置的 Rust Hook Helper
  -> Unix Socket（macOS/Linux）或 Named Pipe（Windows）
  -> 单会话状态归约器
  -> 全局优先级仲裁器
  -> Tauri 控制面板与红绿灯浮窗
```

生产状态通道不依赖 HTTP 端口。历史 Hook Hub 使用的 `8765`、`8766` 等
端口与 Tauri 状态链路相互独立，本应用不依赖这些端口运行。

## Windows 安装

从 [Agent Activity Hub v0.1.1](https://github.com/VICIy/agent-activity-hub/releases/tag/v0.1.1)
下载 64 位 Windows 安装程序：

[下载 Agent.Activity.Hub_0.1.1_x64-setup.exe](https://github.com/VICIy/agent-activity-hub/releases/download/v0.1.1/Agent.Activity.Hub_0.1.1_x64-setup.exe)

运行前请使用 Release 中的
[`SHA256SUMS.txt`](https://github.com/VICIy/agent-activity-hub/releases/download/v0.1.1/SHA256SUMS.txt)
校验下载文件。

安装包支持 64 位 Windows 10 和 Windows 11；系统缺少 WebView2 时会通过微软的
Bootstrapper 安装。当前安装包未进行代码签名，Windows SmartScreen 可能需要选择
“更多信息 > 仍要运行”。安装后打开 **Agent Activity Hub**，在控制面板中安装所需
Provider 的 Hook；已经运行的 Provider 应用需要重启。

## macOS 安装

推荐从 [GitHub Releases](https://github.com/VICIy/agent-activity-hub/releases)
下载 DMG。当前 macOS 发布的
[Agent Activity Hub v0.1.2](https://github.com/VICIy/agent-activity-hub/releases/tag/v0.1.2)
提供 Apple Silicon（`arm64`）安装包：

[下载 Agent.Activity.Hub_0.1.2_aarch64.dmg](https://github.com/VICIy/agent-activity-hub/releases/download/v0.1.2/Agent.Activity.Hub_0.1.2_aarch64.dmg)

安装后的应用名称是 **Agent Activity Hub**；红绿灯是应用内的浮动窗口，不是独立应用。
将应用拖入“应用程序”后，请在 Finder 或 Spotlight 中搜索 **Agent Activity Hub**。

当前 DMG 未使用 Apple Developer ID 签名和公证。首次安装时将应用拖入“应用程序”，
再右键选择“打开”。如果 Gatekeeper 仍提示“文件已损坏”，请先确认下载文件的
SHA-256，再按[未签名 DMG 安装说明](docs/macos-unsigned-install-cn.md)清理隔离标记并
在本机重新生成 ad-hoc 签名。Intel Mac（`x86_64`）需要单独构建 Intel 版本。

## AI Skill 安装

仓库内置了用于自动安装、更新和启动应用的 Skill：
[`skills/agent-activity-hub-install/`](skills/agent-activity-hub-install/)。将该目录安装
到 AI 的 Skill 目录后，可以使用：

```text
Use $agent-activity-hub-install to install Agent Activity Hub on this Mac.
```

该 Skill 会优先选择 GitHub Release；没有可用 Release 时回退到源码构建，并在应用启动
后引导用户在 Tauri 控制面板中安装 Codex、Claude Code、Qoder Hook。

## 功能

- 使用 `provider + instance_id + session_id` 隔离每个会话。
- 支持多个 Agent、多个项目、多个会话和自定义 Provider 并发运行。
- 在控制面板和浮窗展开面板中展示 Agent 名称、项目名称、状态和会话信息。
- 全局展示优先级为
  `异常 > 待审批 > 已完成 > 工作中 > 空闲`。
- 待审批和异常不会自动进入空闲，必须由真实 Provider 事件或显式关闭操作
  改变状态。
- 只有已完成状态会在展示租约结束后自动进入空闲。
- 会话列表为空，或只剩离线/休眠会话时，全局状态统一显示为空闲，三个灯
  全灭。
- 红、绿、黄以及空闲、离线会话都可以单独移除；同一会话产生新事件后会以最新状态
  重新出现。
- 支持英文与简体中文。
- 支持 ESP32-C3 外接红绿灯：桌面端通过 USB 串口实时同步状态和灯效，仓库内置同时支持 USB 与 BLE 的固件。
- 关闭主窗口时应用继续在后台运行，可通过 macOS 程序坞图标或托盘菜单重新
  打开控制面板，也可从托盘菜单恢复红绿灯浮窗。

## 平台兼容性

Windows 使用当前用户的 Named Pipe、兼容 PowerShell 的 Hook Helper 路径引用方式和
Windows Tauri 启动器；macOS 继续使用 Unix Socket、POSIX 可执行权限和原生 Tauri
启动器。平台专用逻辑均通过编译期条件隔离，CI 会在 `windows-2022` 与 `macos-14`
上分别构建、测试并运行 Clippy。

这些改动提交到仓库后，其他用户拉取对应提交并重新构建或安装该版本即可生效；仅拉取
源码不会自动更新已经安装的应用。升级后从控制面板重新安装 Provider Hook，会把按内容
寻址的 Hook Helper 保存到用户的应用数据目录，Hook 不再依赖源码目录或 Rust
`target/` 目录。

## 红绿灯浮窗

ESP32-C3 的固件、刷写、接线和协议说明位于
[`firmware/esp32-traffic-light/`](firmware/esp32-traffic-light/README-cn.md)。刷写后在控制面板
“设置 > ESP32 设备”中刷新端口并连接；硬件使用与浮窗相同的灯效配置，不依赖旧的 `8765` 端口。

默认硬件接线兼容 [GFlash6/minic](https://github.com/GFlash6/minic) ESP32-C3
红绿灯板。GPIO7 公共阳极及 GPIO10/9/8 三灯接线定义参考该项目；本仓库中的
Agent Activity Hub 串口协议、桌面端同步与固件实现由本项目维护。感谢原作者
[GFlash6](https://github.com/GFlash6) 公开硬件项目和接线信息。

灯的顺序固定为绿、黄、红。默认灯效如下：

| 状态 | 默认灯效 | 自动流转 |
|---|---|---|
| 空闲 | 全灭 | 无 |
| 工作中 | 绿灯常亮 | 由后续 Provider 事件决定 |
| 待审批 | 黄灯闪烁，亮灭阶段各 500ms | 不自动进入空闲 |
| 已完成 | 绿灯闪烁，亮灭阶段各 500ms | 完成展示结束后进入空闲 |
| 异常 | 红灯闪烁，亮灭阶段各 500ms | 不自动进入空闲 |
| 离线 / 休眠 | 全灭 | 仅作为单会话诊断状态保留 |

在“设置”页面中可以调整：

- 浮窗纵向或横向布局；
- 每种状态对应的亮灯组合；
- 是否闪烁及闪烁阶段时长；
- 全局灯光亮度；
- 开机自启；
- 界面语言。

浮窗中的 Agent 固定位采用水平流式布局，空间不足时自动换行。展开面板中的
会话卡片分两行展示 Agent 和项目名称，红、绿、黄条目均支持右上角关闭 `x`；离线条目
可在控制面板的“全部会话”列表中移除。关闭条目不会屏蔽后续事件。

## 界面样式与状态展示

### 红绿灯浮窗

浮窗是无边框、透明、置顶的小窗口，默认采用横向布局，三个灯排列在一行；切换为纵向后，三个灯
改为纵向排列。灯罩使用深色圆角外壳，灯珠带有对应颜色的光晕；闪烁时采用
亮灭分明的阶梯动画，默认亮、灭阶段各 500ms。浮窗本体不显示快捷方式，主要
内容从上到下为红绿灯、活跃 Agent 流式条和可展开按钮。

不同状态的浮窗表现如下：

| 状态 | 灯珠 | Agent 固定位 | 会话卡片 |
|---|---|---|---|
| 空闲 | 三灯全灭 | 灰色“空闲” | 默认不保留空闲条目，手动保留时为灰色 |
| 工作中 | 绿灯常亮 | 绿色 Agent 卡片 | 绿色边框和背景 |
| 待审批 | 黄灯闪烁 | 黄色 Agent 卡片 | 黄色边框和背景，不自动消失 |
| 已完成 | 绿灯闪烁 | 绿色 Agent 卡片 | 绿色边框和背景，展示租约结束后回到空闲 |
| 异常 | 红灯闪烁 | 红色 Agent 卡片 | 红色边框和背景，右上角提供关闭 `x` |
| 离线 / 休眠 | 三灯全灭 | 灰色信息 | 仅作为诊断状态保留 |

活跃 Agent 卡片支持多个提供方同时出现，并显示 Agent 名称和会话数量；空间
不足时自动换行。展开“会话”后，面板宽度与浮窗一致，条目使用紧凑卡片布局：
第一行是状态圆点、Agent 名称和状态标签，第二行是项目名称。异常、空闲和离线
条目的关闭 `x` 位于卡片右上角，不占用文字区域；关闭只移除当前展示，不会屏蔽
同一会话后续的新事件。

### Tauri 控制面板

控制面板采用深色石墨色界面，以绿色、黄色、红色作为状态强调色，包含以下页面：

- **总览**：较大的红绿灯、当前全局状态和提供方、待关注数量、事件统计、实时
  会话列表以及适配器摘要。全局状态按
  `异常 > 待审批 > 已完成 > 工作中 > 空闲` 仲裁。
- **会话**：全部会话表格，展示 Agent/会话 ID、项目名称、状态、原因和修订号；
  页面提供返回总览按钮，当前可见的异常、工作中、待审批、完成、空闲和离线条目均可用右上角 `x` 移除。
- **适配器**：检测 Codex、Claude Code、Qoder 的 Hook 配置，显示安装事件数量、
  配置路径和 Helper 状态，并提供一键安装、修复/重装和卸载。
- **诊断**：显示已接受事件、去重事件和本地 IPC 端点状态，并提供工作中、待审批、
  已完成、异常的输出测试按钮。
- **设置**：中英文切换、浮窗纵向/横向切换、开机自启，以及每种状态的灯珠组合、
  闪烁开关、闪烁阶段时长和全局亮度。灯光设置以状态卡片呈现，可直接预览绿、黄、
  红三颗灯的组合。

浮窗五种主要状态的当前样式：

![红绿灯浮窗状态截图](docs/images/floating-light-states.png)

Tauri 控制面板总览和设置页面：

![Tauri 控制面板截图](docs/images/tauri-control-panel.png)

## Provider 适配器

在 Tauri 控制面板中打开“适配器”，可以检测、安装、修复或卸载应用管理的
Hook。

| Provider | 配置文件 | 输入链路 |
|---|---|---|
| Codex | `~/.codex/hooks.json` | 原生 Hook，加结构化会话日志补偿 |
| Claude Code | `~/.claude/settings.json` | 原生 Hook，加结构化会话日志补偿 |
| Qoder | `~/.qoder/settings.json` | 原生 Hook；修复时会移除旧 `flash4-light.sh` 包装脚本 |

应用管理的 Hook 使用
`work.effective.agent-activity-hub/v1` 标识。安装过程会保留其他 Hook 和
顶层设置，并在原子替换配置前创建备份。Codex Hook 不设置工具 matcher，
而是为每个生命周期事件显式传入事件名，确保授权请求能够进入 Tauri。
安装或修复 Hook 后需要重启对应的 Provider，让正在运行的进程加载新配置。

Hook Helper 已随应用打包。最终用户不需要本仓库、Python、私有 Shell 脚本或
固定 HTTP 服务。

仓库中还提供 Hook 配置诊断：

```bash
node tools/codex_hooks.mjs doctor
node tools/claude_hooks.mjs doctor
node tools/qoder_hooks.mjs doctor
```

## 开发运行

环境要求：

- Rust 1.77 或更高版本；
- Node.js 22 或更高版本；
- npm 10 或更高版本；
- Tauri 2 对应平台的系统依赖。

```bash
cd apps/agent-activity-desktop
npm install
npm run tauri dev
```

开发启动器从 `127.0.0.1:1420` 开始查找可用的 Vite 地址。端口被占用时会
自动选择下一个可用端口，并把同一个地址传给 Vite 和 Tauri。

## 生产构建

在 Windows 上生成 NSIS 安装程序：

```powershell
cd apps/agent-activity-desktop
npm run tauri build -- --bundles nsis
```

在 macOS 上生成 DMG：

```bash
npm run tauri build -- --bundles dmg
```

构建流程会编译当前目标平台的 Rust Hook Helper，复制到 Tauri sidecar 目录，
构建 React 前端并打包桌面应用。各平台安装包输出位置：

```text
target/release/bundle/nsis/Agent Activity Hub_0.1.1_x64-setup.exe
target/release/bundle/macos/Agent Activity Hub.app
target/release/bundle/dmg/Agent Activity Hub_0.1.2_aarch64.dmg
```

启动打包后的应用：

```bash
open -n "target/release/bundle/macos/Agent Activity Hub.app"
```

macOS 应用包中包含圆角应用图标以及内置的 `agent-activity-hook`。

## 验证

运行 Rust 与前端测试：

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

cd apps/agent-activity-desktop
npm run test -- --run
npm run build
```

启动生产版 Tauri 应用后，可以执行生命周期测试脚本。脚本通过内置 Hook
Helper 发送事件，并检查 Tauri 持久化状态，覆盖多个 Provider、项目、会话，
串行状态流转、并发状态仲裁、审批同意/拒绝、异常保持、完成租约、离线恢复
以及最终回到空闲。

```bash
tools/verify_multi_agent_lifecycle.zsh
tools/verify_concurrent_multistate.zsh
```

测试脚本需要 `sqlite3` 和 `jq`，并会在发现无关的活跃工作流时拒绝执行，
避免覆盖真实会话状态。

## 仓库结构

```text
apps/agent-activity-desktop/       React 界面与 Tauri 壳
native/agent-activity/             协议、状态机、IPC、存储、Hook Helper
sdk/protocol-schema/               公共 JSON Schema
fixtures/agent_activity/           脱敏后的 Provider 事件样例
tools/                              启动、Hook 维护与验证工具
docs/                               Provider 支持与实施状态
```

运行时数据保存在各平台的应用数据目录：

```text
Windows: %LOCALAPPDATA%\Effective Work\Agent Activity Hub\data\
macOS:   ~/Library/Application Support/work.Effective-Work.Agent-Activity-Hub/
```

该目录包含 SQLite 事件/状态数据库、持久化的 Hook Helper 和本地 IPC 状态。
Provider 原始载荷会先被标准化，敏感的工具输入不会写入数据库。

## 生成文件清理

仓库会忽略 `target/`、`dist/`、`node_modules/`、本地数据库和日志。
请使用受保护的清理命令，不要直接删除 Rust 的 target 目录：

```bash
node tools/clean_generated.mjs --dry-run
node tools/clean_generated.mjs
```

清理工具会保留生产应用、内置 Hook Helper、sidecar，以及已安装 Provider
Hook 仍在引用的兼容性可执行文件。

更多实现细节见
[docs/architecture-cn.md](docs/architecture-cn.md)、
[docs/implementation-status.md](docs/implementation-status.md) 和
[docs/provider-support.md](docs/provider-support.md)。
