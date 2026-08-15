<h1 align="center">
  <img src="public/mangodisk.svg" width="40" alt="MangoDisk 应用图标"> MangoDisk 芒果磁盘清理
</h1>

<p align="center">
  <a href="README.md">English</a> · 简体中文 · <a href="README.zh-TW.md">繁體中文</a> · <a href="README.ja.md">日本語</a>
</p>

<p align="center">
  <a href="https://github.com/harry0703/MangoDisk/releases/latest"><img alt="最新版本" src="https://img.shields.io/github/v/release/harry0703/MangoDisk?display_name=tag&sort=semver"></a>
  <img alt="支持 macOS" src="https://img.shields.io/badge/macOS-supported-111827?logo=apple&logoColor=white">
  <img alt="支持 Windows" src="https://img.shields.io/badge/Windows-supported-2563eb?logo=windows&logoColor=white">
  <img alt="Tauri 2" src="https://img.shields.io/badge/Tauri-2-24c8db?logo=tauri&logoColor=white">
  <img alt="Rust Core" src="https://img.shields.io/badge/core-Rust-b7410e?logo=rust&logoColor=white">
</p>

<p align="center">
  <a href="https://mangodisk.app/zh">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/readme/zh-dark.jpg">
      <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/readme/zh-light.jpg">
      <img src="https://assets.mangodisk.app/images/readme/zh-light.jpg" width="1200" alt="MangoDisk 深度清理磁盘，释放更多空间">
    </picture>
  </a>
</p>

## MangoDisk 能做什么

### 深度清理

深度清理是 MangoDisk 的核心功能。它会集中扫描系统、应用、开发环境和本地项目中占用空间的内容，并按类别汇总可释放空间：

- **系统与应用**：系统和用户临时文件、应用缓存、浏览器缓存、诊断日志以及已卸载应用的残留文件。
- **开发工具**：包管理器下载缓存、IDE 索引、开发工具临时文件，以及 Xcode 和 Windows 开发环境生成的数据。
- **项目构建产物**：Node.js 依赖和框架缓存、Rust `target` 目录，以及 Gradle、Swift、Python、.NET、Godot 和 CMake 等工具生成的可重建内容。
- **AI 模型与缓存**：本地下载的 AI 模型和临时传输文件，帮助定位占用空间较大的模型数据。
- **应用优化**：识别当前设备不需要的处理器代码，在支持的应用中进一步减少空间占用。

扫描阶段只读取文件信息，不会自动删除内容。结果会显示类别、大小、位置和风险提示；你可以使用智能推荐，也可以逐项检查和选择，确认可释放空间后再执行清理。

### 大文件清理

扫描磁盘或指定文件夹，按视频、音频、图片、压缩包、文档和安装包等类型查看大文件。你可以设置最小文件大小，按占用空间排序，并在文件管理器中打开位置确认内容，再选择不需要的文件进行清理。

### 重复文件清理

扫描选定的文件夹，按文件内容确认完全相同的副本，而不是只比较文件名。结果会按组显示副本数量、单个文件大小和最多可释放空间，并通过智能选择在每组至少保留一份文件。

### 应用卸载与残留清理

查看已安装应用的大小、状态和相关文件，卸载应用时可同时检查缓存、偏好设置和应用私有数据。MangoDisk 会区分卸载必选内容、可重新生成的数据和可能包含用户文件的内容，并提示正在运行或受保护的应用。

### 磁盘空间分析

分析磁盘或指定文件夹，通过矩形图和列表查看目录、文件数量与空间占用。你可以逐层浏览文件夹，快速定位占用最多的目录和文件，并直接在系统文件管理器中打开对应位置。

## 清理规则与安全性

MangoDisk 维护自己的跨平台清理规则库，不会直接照搬第三方项目的规则。Windows 规则会参考 Winapp2.ini 发现候选路径，macOS 规则也会参考相关开源项目，但这些信息只作为研究线索，不能直接成为清理依据。

候选规则进入正式版本前，必须完成以下检查：

- **核对可靠来源**：通过 Microsoft、Apple 或软件厂商的官方资料确认路径用途和数据归属。
- **确认清理边界**：判断内容是否可以安全重建，排除个人文件、应用私有数据和系统保护路径。
- **完成实机验证**：在规则对应的 Windows 或 macOS 环境中验证路径、清理结果和异常场景。

只有通过来源核对、安全审查和实机验证的规则，才会加入正式规则库。
简单来说：**会参考第三方项目提供线索，但必须经过官方证据和实测结果决定是否采用。**

完整规则库已公开，规则内容和修改记录均可审计、追溯：[查看 MangoDisk 清理规则库](https://github.com/harry0703/MangoDisk/tree/main/src-tauri/crates/mangodisk-core/rules)。

MangoDisk 始终把数据安全放在清理效果之前：无法明确确认安全边界的内容不会纳入正式规则，清理内容也会在执行前展示并由用户确认。

## 界面预览

<p align="center">
  <strong>深度清理</strong><br>
  <sub>集中扫描系统、应用、开发工具和项目中的可清理内容，确认后统一处理。</sub>
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/screenshots/zh/dark-01-deep-cleanup.jpg">
    <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/screenshots/zh/light-01-deep-cleanup.jpg">
    <img src="https://assets.mangodisk.app/images/screenshots/zh/light-01-deep-cleanup.jpg" width="1200" alt="MangoDisk 深度清理界面">
  </picture>
</p>

<table>
  <tr>
    <td width="50%" align="center">
      <strong>大文件清理</strong><br>
      <sub>按类型和大小查找大文件，确认后再进行清理。</sub><br><br>
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/screenshots/zh/dark-02-large-file-cleanup.jpg">
        <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/screenshots/zh/light-02-large-file-cleanup.jpg">
        <img src="https://assets.mangodisk.app/images/screenshots/zh/light-02-large-file-cleanup.jpg" width="100%" alt="MangoDisk 大文件清理界面">
      </picture>
    </td>
    <td width="50%" align="center">
      <strong>重复文件清理</strong><br>
      <sub>按内容查找完全相同的文件，并保留至少一份。</sub><br><br>
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/screenshots/zh/dark-03-duplicate-cleanup.jpg">
        <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/screenshots/zh/light-03-duplicate-cleanup.jpg">
        <img src="https://assets.mangodisk.app/images/screenshots/zh/light-03-duplicate-cleanup.jpg" width="100%" alt="MangoDisk 重复文件清理界面">
      </picture>
    </td>
  </tr>
  <tr>
    <td width="50%" align="center">
      <strong>应用卸载与残留清理</strong><br>
      <sub>卸载应用，并检查缓存、设置和应用私有数据。</sub><br><br>
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/screenshots/zh/dark-04-app-uninstaller.jpg">
        <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/screenshots/zh/light-04-app-uninstaller.jpg">
        <img src="https://assets.mangodisk.app/images/screenshots/zh/light-04-app-uninstaller.jpg" width="100%" alt="MangoDisk 应用卸载界面">
      </picture>
    </td>
    <td width="50%" align="center">
      <strong>磁盘空间分析</strong><br>
      <sub>通过矩形图和列表定位占用空间最多的内容。</sub><br><br>
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/screenshots/zh/dark-05-disk-space-analysis.jpg">
        <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/screenshots/zh/light-05-disk-space-analysis.jpg">
        <img src="https://assets.mangodisk.app/images/screenshots/zh/light-05-disk-space-analysis.jpg" width="100%" alt="MangoDisk 磁盘空间分析界面">
      </picture>
    </td>
  </tr>
</table>

## 安装与使用

macOS 用户可以通过 Homebrew 快速安装：

```sh
brew install --cask harry0703/tap/mangodisk
```

也可以前往 [MangoDisk 官网](https://mangodisk.app/zh) 或 [GitHub Releases](https://github.com/harry0703/MangoDisk/releases/latest) 下载最新版：

- **macOS**：打开 DMG，将 MangoDisk 拖入“应用程序”文件夹。
- **Windows**：运行 Windows 安装程序并按提示完成安装。

> [!IMPORTANT]
> 清理和卸载属于破坏性操作。请在删除个人文件前确认内容，并为重要数据保留可靠备份。

## CLI 快速示例

macOS 用户可以通过 Homebrew 安装独立 CLI：

```sh
brew install harry0703/tap/mangodisk-cli
```

Windows 用户可以通过 WinGet 安装：

```powershell
winget install --id MangoDisk.CLI -e
```

两种安装方式都会将 `mangodisk` 加入命令路径。如果安装后暂时无法识别命令，请重新打开终端，然后检查版本：

```sh
mangodisk --version
```

CLI 与桌面应用使用同一套安全清理引擎，可以使用以下命令：

```sh
# 只扫描并展示可清理内容
mangodisk clean

# 应用与桌面端一致的智能推荐
mangodisk clean --apply

# 预览全部可选内容，不实际删除
mangodisk clean --apply --selection all --dry-run

# 输出便于脚本处理的 JSON
mangodisk clean --format json --no-progress
```

默认命令只扫描。非交互环境执行清理时还需要明确确认参数；完整选项请运行：

```sh
mangodisk clean --help
```

## 从源码构建

### 环境要求

- Node.js 24 LTS
- pnpm 11.13.1
- Stable Rust
- macOS：Xcode Command Line Tools
- Windows：Visual Studio 2022 Build Tools，并安装“使用 C++ 的桌面开发”
- Windows：Microsoft Edge WebView2 Runtime

平台依赖也可以参考 [Tauri 2 前置依赖说明](https://v2.tauri.app/start/prerequisites/)。

### 获取源码并启动桌面应用

```sh
git clone https://github.com/harry0703/MangoDisk.git
cd MangoDisk
pnpm install --frozen-lockfile
pnpm tauri:dev
```

### 运行完整检查

```sh
pnpm check
cargo test --manifest-path src-tauri/Cargo.toml -p mangodisk-core
```

### 构建桌面安装包

```sh
pnpm tauri:build
```

### 构建 CLI

```sh
pnpm cli:build
```

本地构建产物不包含 MangoDisk 正式发布流程提供的签名、公证和更新元数据，仅用于开发与验证。

## 参与贡献

欢迎提交问题、清理规则、修复和新功能。开始前请阅读
[`CONTRIBUTING.md`](CONTRIBUTING.md) 和 [`AGENTS.md`](AGENTS.md)。

常规清理覆盖优先使用经过构建期校验的声明式 TOML 规则。规则结构、安全约束和验证方式请参阅
[`src-tauri/crates/mangodisk-core/rules/README.md`](src-tauri/crates/mangodisk-core/rules/README.md)。

提交修改前，请至少运行：

```sh
pnpm check
cargo test --manifest-path src-tauri/Cargo.toml -p mangodisk-core
```

发现安全问题时，请按照 [`SECURITY.md`](SECURITY.md) 通过 GitHub Security Advisories 私下报告，不要创建公开 Issue。

## 技术栈

- [Tauri 2](https://tauri.app/)：桌面运行时与系统集成
- [Rust](https://www.rust-lang.org/)：扫描、文件系统、安全校验和清理执行
- [Vue 3](https://vuejs.org/) 与 [TypeScript](https://www.typescriptlang.org/)：桌面交互界面

## 许可证

MangoDisk 基于 [GNU General Public License v3.0](https://github.com/harry0703/MangoDisk/blob/main/LICENSE) 开源。第三方组件继续遵循各自的许可证。
