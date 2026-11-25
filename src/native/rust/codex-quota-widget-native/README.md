# codex用量监测 Rust 0.2.9

Windows x64 原生版本，使用 Win32、GDI+ 和 WinHTTP。EXE 内嵌人物、翅膀及托盘图标，运行无需 .NET。

## 使用

- 正常启动 EXE 后会查找 Codex 窗口。找不到时托盘提示「等待 Codex 窗口」。Codex 最小化时隐藏悬浮窗，检测到所跟随的 Codex 进程退出后退出。
- 人物下方默认显示 5 小时和周额度。单击打开详情，拖动移动。托盘菜单可切换主题、样式以及每种样式的独立尺寸。
- 按 `--demo` 启动可在没有 Codex 的情况下使用模拟额度。演示模式不读取登录状态。
- 按 `--watch` 启动可在 Codex 退出后回到等待状态，随后重新跟随新开的 Codex。安装脚本可选创建 Windows 登录启动项启用该模式。

## 数据

真实额度由 WinHTTP 从固定 HTTPS 额度端点只读查询。请求从当前用户的 Codex 登录文件获取登录信息，禁止跟随重定向。登录信息及响应正文不写入设置或日志。正常刷新间隔为每次查询完成后 30 秒，限流时延长至 120 秒。失败时可在托盘菜单查看提示，已有额度带星号表示旧数据。

今日用量从本机会话记录汇总，按本地日历日计算，使用 k 或 M 缩写。设置文件只保存主题、样式和尺寸。

## 构建

在本目录使用 Windows x64 MSVC Rust 工具链。

```powershell
cargo test --locked
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --release --locked
```

将 Release 产物复制到 `artifacts/rust/mascot-v0.2.9/CodexQuotaWidget-Rust-0.2.9-x64.exe`。在已安装 Inno Setup 6 的机器上，可以用 `ISCC.exe installer/CodexQuotaWidget-Rust.iss` 从仓库根目录编译安装器。
