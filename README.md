# DeepSeek Harness 桌面版（Tauri 2 封装）

把官方 `dsh web` 封装成 Windows 桌面应用：Tauri 壳托管 dsh 进程生命周期（启动→等端口就绪→加载页面→退出时优雅清理），双击即用，无需手动开终端。

## 设计原则
- **不改 DSH 源码**：壳是纯外层包装，只 spawn 官方 `dsh web`，不 import/patch 任何 DSH 文件。
- **版本锁 + 手动升级**：预装固定版 dsh + 便携 Node 进安装包资源目录，启动时 `node <bundle>/bin.js web`。不走 npx（npx 默认拉最新，不可控）。
- **零用户依赖**：便携 Node 打包进安装包，用户无需自装 Node.js。双击安装包即用。

## 项目定位与生命周期
本项目是 **DSH 预览期的第三方桌面封装**，核心价值是在官方没有桌面版时让普通用户零门槛用上 DSH。

**当 DeepSeek 推出官方桌面版后，本项目将停止维护**——官方版必然比第三方壳更权威（原生集成、官方更新通道、品牌一致），继续维护只会徒增用户困惑。届时：
- 仓库 README 顶部会置顶迁移公告，引导用户切换到官方版。
- 最后一个版本保持可用，不再发新版。
- 代码开源保留，作为 Tauri 包装 Node 后端进程的参考实现。

现阶段的更新策略：DSH 预览版仍在迭代，本项目**锁版本**（DSH_PINNED_VERSION 常量），避免官方新版有 bug 时波及用户。DSH 新版需手动改常量 + 重新打包才会下发。

## 架构
- **壳**：Tauri 2.x，仅一个静态 splash 启动页，dsh 就绪后 `eval` 跳转到 `http://127.0.0.1:<动态端口>/`。
- **运行时**：便携 Node（`resources/node-portable/node.exe`）+ 预装 dsh（`resources/dsh-bundle/`），都打进安装包。
- **进程管理**：Rust 原生 `std::process::Command`（不用 `tauri-plugin-shell`，其 kill 是硬杀且杀不到孙进程 node）。
  - `<node.exe> <bundle>/bin.js web --host 127.0.0.1 --port <动态端口>`，`CREATE_NEW_PROCESS_GROUP`。
  - 优雅退出：`AllocConsole`（隐藏）+ `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, node_pid)` 触发 node 的 SIGINT → dsh 5s drain；超时则 `TerminateProcess` 兜底。
  - 异常兜底：Job Object（`KILL_ON_JOB_CLOSE`），Tauri 崩溃时内核自动回收整树，无残留端口。
- **单实例**：`tauri-plugin-single-instance`。
- **数据目录**：`%APPDATA%\DeepSeekHarness\dsh-home`（或 `$DSH_HOME`）。日志 `dsh-<ts>.log`。

## 插件生态不受影响（保证）
DSH 的插件机制完全保留，原因：
1. **DSH_HOME 独立可写**：指向 `%APPDATA%\DeepSeekHarness\dsh-home`，不指向只读的 bundle 目录。DSH 的 profile 初始化到 `$DSH_HOME/profiles/web`，用户装的插件存在 `$DSH_HOME/profiles/web/node_modules`。
2. **bundle 只读但不干涉解析**：DSH 的标准插件解析顺序是"先 dsh 安装目录（我们的 bundle），后 profile 的 node_modules"。内置 bundle 与用户插件互不覆盖。
3. **不 patch dsh 的 plugin 逻辑**：壳只调 `dsh web`，`dsh plugin --profile web add <package>` 等命令照常可用。
4. **菜单"打开数据目录"**：方便用户在文件管理器查看 profile/插件/日志。

## 升级流程
升级 DSH 或 Node 时，改 `src-tauri/src/dsh.rs` 的 `DSH_PINNED_VERSION` / `NODE_PINNED_VERSION` 常量，跑 `fetch-dsh.ps1` 重拉，`cargo tauri build` 重打包。`DSH_HOME` 数据目录保留（会话/凭据/插件不丢）。

## 前置条件
### 构建者
1. **Rust**：[rustup](https://rustup.rs/) 装 `stable-x86_64-pc-windows-msvc`。
2. **MSVC C++ 构建工具**：VS Build Tools 2022 + "Desktop development with C++"（MSVC v143 + Windows 11 SDK）。
3. **Tauri CLI**：`cargo install tauri-cli --version "^2"`。
4. **Node.js**（用于跑 `fetch-dsh.ps1` 拉取 dsh bundle + 便携 Node）。

### 最终用户
- **零依赖**：便携 Node 已打包，无需自装。WebView2 在 Win11 自带。

## 构建与运行
```powershell
# 1. 预装 dsh bundle + 便携 Node（首次或升级时）
pwsh scripts\fetch-dsh.ps1

# 2. 开发运行
cd src-tauri
cargo tauri dev

# 3. 打包 NSIS 安装包（需先生成图标：cargo tauri icon <png>）
cargo tauri build
```

## 验证清单
- 启动后窗口标题 "DeepSeek Harness"，先显 splash，后跳 dsh UI。
- Settings → Models 配 DeepSeek API key，跑一个 trivial 任务。
- `tasklist | findstr node` 应见一个 node.exe（便携 Node）。
- 点 × 关闭：≤6s 内 node 消失、端口释放。
- Task Manager 强杀 Tauri：node 被 Job Object 回收。
- 二次启动：聚焦已有窗口，不启第二个 dsh。
- 菜单"关于"显示内置 DSH + Node 版本。
- 菜单"打开数据目录"打开 `%APPDATA%\DeepSeekHarness\dsh-home`。

## 限制（v0.1）
- Node 版本由打包锁定，不跟随用户系统升级（与 dsh 锁版本一致，可接受）。
- 仅 Windows。
- 安装包体积：便携 Node ~30MB + dsh bundle ~50-100MB = 总约 80-130MB。
