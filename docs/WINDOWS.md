# StudyPulse Windows 构建与封装

Windows 版本复用与 macOS 相同的 React 前端、Tauri 宿主和 Rust Core。Workspace 格式不分平台；Windows 仅增加平台安装配置、系统文件打开方式、Windows Credential Manager 凭据后端和本机 Python 探测。

## 支持范围

- Windows 10 / 11，当前默认构建宿主架构（通常为 x64）
- NSIS `.exe` 安装器与 WiX `.msi` 安装器
- WebView2 使用安装时下载引导程序；目标机器安装时需要联网，已存在合适 WebView2 Runtime 时不会重复安装
- Cloud AI 与 BYOK 凭据由 Windows Credential Manager 保存

## 构建环境

安装 Node.js 24+、npm 11+、Rust 1.97.1+、Visual Studio 2022 Build Tools 的“使用 C++ 的桌面开发”工作负载、Windows SDK 与 WebView2 Runtime。

## 开发运行

```powershell
npm ci
npm run tauri:dev
```

浏览器中的 `npm run dev` 仍只是前端预览，不能验证 Workspace、文件对话框、系统凭据、深链或 Agent command。

## 生成安装器

```powershell
npm run tauri:build:windows
```

脚本会在缺少 `node_modules` 时先执行 `npm ci`，随后生成：

- `src-tauri/target/release/bundle/nsis/*.exe`
- `src-tauri/target/release/bundle/msi/*.msi`

只构建一种安装器：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-windows.ps1 -Bundles nsis
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-windows.ps1 -Bundles msi
```

## 从 macOS 迁移 Workspace

推荐在 macOS 版本导出 `.studypulsebackup`，再在 Windows 版本中导入。也可以复制完整 Workspace 目录，但必须保留目录结构。不要在 Workspace 内放置符号链接或 junction；路径安全检查会拒绝 link-like 组件。

系统凭据不会随 Workspace 或备份迁移。首次在 Windows 打开后，需要重新登录 Cloud AI 或重新填写 BYOK API key。

## Agent 本机 Python

Windows 会按 `PATH` 探测 `python.exe`、`python3.exe` 和 `py.exe`。如果 Python 不在 `PATH`，可通过 `STUDYPULSE_PYTHON` 指定完整路径。本机 Python 执行仍不是安全沙箱；需要隔离时使用 Docker Runner。

## 发布签名

本地构建默认不签名，安装时可能出现 Microsoft Defender SmartScreen 提示。正式分发前应使用受信任的 Windows 代码签名证书对主程序、NSIS 和 MSI 产物签名，并配置时间戳服务。证书、私钥和密码不得提交到仓库或写入 Workspace。
