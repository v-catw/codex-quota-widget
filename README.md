# Codex Quota Widget · Codex 用量监测

一个 Windows 桌面悬浮窗，用来查看 Codex 的 5 小时和每周剩余额度、重置时间，以及本机今日 token 用量。

## 安装和使用

当前版本为 **Rust 0.2.9**，支持 Windows x64，无需安装 .NET。提供中英文安装向导，也可直接运行免安装版。

安装包与免安装版通过 [GitHub Releases](https://github.com/v-catw/codex-quota-widget/releases) 发布。选择文件名带 `Setup-x64.exe` 的安装包，或选择 `CodexQuotaWidget-Rust-<版本>-x64.exe` 直接运行。

打开 Codex 后启动组件，悬浮窗会跟随 Codex 窗口。拖动可移动位置，点击任务栏通知区域的图标可切换样式、主题和尺寸。

| 样式 | 展示与交互 |
| --- | --- |
| GPT 娘 | 人物下方常显两项额度，点击人物打开详情气泡 |
| 双环 | 外环表示 5 小时额度，内环表示每周额度，悬停查看详情 |
| 横向 | 并列显示两项额度，右侧图标切换日间与夜间主题 |

覆盖安装前，从托盘退出正在运行的 Rust 版。同一安装目录中的旧版程序会被替换。独立下载的免安装 EXE 和旧 C# 版可以单独使用。

## 数据来源

账户剩余额度通过 HTTPS 只读查询，常规刷新间隔为 30 秒。今日 token 用量按本地日期从这台电脑的 Codex 会话记录汇总。登录凭据仅用于额度查询，设置和日志不保存凭据。

## 开发

Rust 版使用 Win32、GDI+ 和 WinHTTP。使用 Windows x64 MSVC 工具链，在仓库根目录执行：

```powershell
cargo test --locked
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --release --locked
```

生成的 EXE 位于 `target/release/codex-quota-widget-native.exe`。在没有启动 Codex 时，可以给 EXE 传入 `--demo` 来预览界面。使用 `--watch` 时，组件会在 Codex 退出后等待下一次启动。

构建安装包时，将 Release EXE 复制到 `artifacts/rust/mascot-v0.2.9/CodexQuotaWidget-Rust-0.2.9-x64.exe`，再从仓库根目录运行 `ISCC.exe installer/CodexQuotaWidget-Rust.iss`。需要 Inno Setup 6。

这是一个独立开发的工具。
