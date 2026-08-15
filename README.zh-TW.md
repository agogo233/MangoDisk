<h1 align="center">
  <img src="public/mangodisk.svg" width="40" alt="MangoDisk 應用程式圖示"> MangoDisk
</h1>

<p align="center">
  <a href="README.md">English</a> · <a href="README.zh-CN.md">简体中文</a> · 繁體中文 · <a href="README.ja.md">日本語</a>
</p>

<p align="center">
  <a href="https://github.com/harry0703/MangoDisk/releases/latest"><img alt="最新版本" src="https://img.shields.io/github/v/release/harry0703/MangoDisk?display_name=tag&sort=semver"></a>
  <img alt="支援 macOS" src="https://img.shields.io/badge/macOS-supported-111827?logo=apple&logoColor=white">
  <img alt="支援 Windows" src="https://img.shields.io/badge/Windows-supported-2563eb?logo=windows&logoColor=white">
  <img alt="Tauri 2" src="https://img.shields.io/badge/Tauri-2-24c8db?logo=tauri&logoColor=white">
  <img alt="Rust Core" src="https://img.shields.io/badge/core-Rust-b7410e?logo=rust&logoColor=white">
</p>

<p align="center">
  <a href="https://mangodisk.app/tw">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/readme/tw-dark.jpg">
      <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/readme/tw-light.jpg">
      <img src="https://assets.mangodisk.app/images/readme/tw-light.jpg" width="1200" alt="MangoDisk 深度清理磁碟，釋放更多空間">
    </picture>
  </a>
</p>

## MangoDisk 能做什麼

### 深度清理

深度清理是 MangoDisk 的核心功能。它會集中掃描系統、應用程式、開發環境和本機專案中佔用空間的內容，並依類別彙整可釋放空間：

- **系統與應用程式**：系統和使用者暫存檔案、應用程式快取、瀏覽器快取、診斷記錄檔以及已解除安裝應用程式的殘留檔案。
- **開發工具**：套件管理工具下載快取、IDE 索引、開發工具暫存檔案，以及 Xcode 和 Windows 開發環境產生的資料。
- **專案建置產物**：Node.js 相依套件和框架快取、Rust `target` 資料夾，以及 Gradle、Swift、Python、.NET、Godot 和 CMake 等工具產生的可重建內容。
- **AI 模型與快取**：本機下載的 AI 模型和暫存傳輸檔案，幫助定位佔用空間較大的模型資料。
- **應用程式最佳化**：識別目前裝置不需要的處理器程式碼，在支援的應用程式中進一步減少空間佔用。

掃描階段只讀取檔案資訊，不會自動刪除內容。結果會顯示類別、大小、位置和風險提示；你可以使用智慧推薦，也可以逐項檢查和選擇，確認可釋放空間後再執行清理。

### 大型檔案清理

掃描磁碟或指定資料夾，依影片、音訊、圖片、壓縮檔、文件和安裝程式等類型查看大型檔案。你可以設定最小檔案大小、依佔用空間排序，並在 Finder 或檔案總管中開啟位置確認內容，再選取不需要的檔案進行清理。

### 重複檔案清理

掃描選取的資料夾，依檔案內容確認完全相同的副本，而不只比較檔名。結果會分組顯示副本數量、單一檔案大小和最多可釋放空間；使用智慧選取時，每組仍會保留至少一份檔案。

### 解除安裝應用程式與殘留清理

查看已安裝應用程式的大小、狀態和相關檔案；解除安裝時，可一併檢查快取、偏好設定和應用程式私有資料。MangoDisk 會區分解除安裝所需項目、可重新產生的資料，以及可能包含使用者檔案的內容，並提示正在執行或受保護的應用程式。

### 磁碟空間分析

分析磁碟或指定資料夾，透過樹狀圖和清單查看資料夾、檔案數量與空間用量。你可以逐層瀏覽資料夾，快速找到佔用最多的資料夾和檔案，並直接在 Finder 或檔案總管中開啟對應位置。

## 清理規則與安全性

MangoDisk 維護自己的跨平台清理規則庫，不會直接照搬第三方專案的規則。Windows 規則會參考 Winapp2.ini 發現候選路徑，macOS 規則也會參考相關開放原始碼專案，但這些資訊僅作為研究線索，不能直接成為清理依據。

候選規則進入正式版本前，必須完成以下檢查：

- **核對可靠來源**：透過 Microsoft、Apple 或軟體廠商的官方資料確認路徑用途和資料歸屬。
- **確認清理邊界**：判斷內容是否可以安全重建，排除個人檔案、應用程式私有資料和系統保護路徑。
- **完成實機驗證**：在規則對應的 Windows 或 macOS 環境中驗證路徑、清理結果和異常場景。

只有透過來源核對、安全審查和實機驗證的規則，才會加入正式規則庫。
簡單來說：**會參考第三方專案提供線索，但必須經過官方佐證和實測結果決定是否採用。**

完整規則庫已公開，每項規則及修改紀錄都可檢視與追溯：[查看 MangoDisk 清理規則庫](https://github.com/harry0703/MangoDisk/tree/main/src-tauri/crates/mangodisk-core/rules)。

MangoDisk 始終把資料安全放在清理效果之前：無法明確確認安全邊界的內容不會納入正式規則，清理內容也會在執行前展示並由使用者確認。

## 介面預覽

<p align="center">
  <strong>深度清理</strong><br>
  <sub>集中掃描系統、應用程式、開發工具和專案中的可清理內容，確認後統一處理。</sub>
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/screenshots/tw/dark-01-deep-cleanup.jpg">
    <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/screenshots/tw/light-01-deep-cleanup.jpg">
    <img src="https://assets.mangodisk.app/images/screenshots/tw/light-01-deep-cleanup.jpg" width="1200" alt="MangoDisk 深度清理介面">
  </picture>
</p>

<table>
  <tr>
    <td width="50%" align="center">
      <strong>大型檔案清理</strong><br>
      <sub>依類型和大小尋找大型檔案，確認後再進行清理。</sub><br><br>
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/screenshots/tw/dark-02-large-file-cleanup.jpg">
        <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/screenshots/tw/light-02-large-file-cleanup.jpg">
        <img src="https://assets.mangodisk.app/images/screenshots/tw/light-02-large-file-cleanup.jpg" width="100%" alt="MangoDisk 大型檔案清理介面">
      </picture>
    </td>
    <td width="50%" align="center">
      <strong>重複檔案清理</strong><br>
      <sub>按內容尋找完全相同的檔案，並保留至少一份。</sub><br><br>
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/screenshots/tw/dark-03-duplicate-cleanup.jpg">
        <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/screenshots/tw/light-03-duplicate-cleanup.jpg">
        <img src="https://assets.mangodisk.app/images/screenshots/tw/light-03-duplicate-cleanup.jpg" width="100%" alt="MangoDisk 重複檔案清理介面">
      </picture>
    </td>
  </tr>
  <tr>
    <td width="50%" align="center">
      <strong>解除安裝應用程式與殘留清理</strong><br>
      <sub>解除安裝應用程式，並檢查快取、設定和應用程式私有資料。</sub><br><br>
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/screenshots/tw/dark-04-app-uninstaller.jpg">
        <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/screenshots/tw/light-04-app-uninstaller.jpg">
        <img src="https://assets.mangodisk.app/images/screenshots/tw/light-04-app-uninstaller.jpg" width="100%" alt="MangoDisk 解除安裝應用程式介面">
      </picture>
    </td>
    <td width="50%" align="center">
      <strong>磁碟空間分析</strong><br>
      <sub>透過樹狀圖和清單定位佔用空間最多的內容。</sub><br><br>
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/screenshots/tw/dark-05-disk-space-analysis.jpg">
        <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/screenshots/tw/light-05-disk-space-analysis.jpg">
        <img src="https://assets.mangodisk.app/images/screenshots/tw/light-05-disk-space-analysis.jpg" width="100%" alt="MangoDisk 磁碟空間分析介面">
      </picture>
    </td>
  </tr>
</table>

## 安裝與使用

macOS 使用者可以透過 Homebrew 快速安裝：

```sh
brew install --cask harry0703/tap/mangodisk
```

也可以前往 [MangoDisk 官網](https://mangodisk.app/tw) 或 [GitHub Releases](https://github.com/harry0703/MangoDisk/releases/latest) 下載最新版：

- **macOS**：開啟 DMG，將 MangoDisk 拖入「應用程式」資料夾。
- **Windows**：執行 Windows 安裝程式並按提示完成安裝。

> [!IMPORTANT]
> 清理和解除安裝會刪除檔案。刪除個人檔案前請確認選取內容，並將重要資料備份到可靠的位置。

## CLI 快速上手

macOS 使用者可以透過 Homebrew 安裝獨立 CLI：

```sh
brew install harry0703/tap/mangodisk-cli
```

Windows 使用者可以透過 WinGet 安裝：

```powershell
winget install --id MangoDisk.CLI -e
```

兩種安裝方式都會將 `mangodisk` 加入命令路徑。如果安裝後暫時找不到命令，請重新開啟終端機，再檢查版本：

```sh
mangodisk --version
```

CLI 與桌面應用程式使用同一套安全清理引擎，可以使用以下命令：

```sh
# 只掃描並展示可清理內容
mangodisk clean

# 套用與桌面應用程式相同的智慧建議
mangodisk clean --apply

# 預覽全部可選內容，不實際刪除
mangodisk clean --apply --selection all --dry-run

# 輸出便於腳本處理的 JSON
mangodisk clean --format json --no-progress
```

預設命令只會掃描。在非互動式環境執行清理時，還需要提供明確的確認參數；完整選項請執行：

```sh
mangodisk clean --help
```

## 從原始碼建置

### 環境要求

- Node.js 24 LTS
- pnpm 11.13.1
- Rust 穩定版工具鏈
- macOS：Xcode Command Line Tools
- Windows：Visual Studio 2022 Build Tools，並安裝「使用 C++ 的桌面開發」
- Windows：Microsoft Edge WebView2 Runtime

各平台的相依套件需求請參考 [Tauri 2 前置需求](https://v2.tauri.app/start/prerequisites/)。

### 取得原始碼並啟動桌面應用程式

```sh
git clone https://github.com/harry0703/MangoDisk.git
cd MangoDisk
pnpm install --frozen-lockfile
pnpm tauri:dev
```

### 執行完整檢查

```sh
pnpm check
cargo test --manifest-path src-tauri/Cargo.toml -p mangodisk-core
```

### 建置桌面安裝程式

```sh
pnpm tauri:build
```

### 建置 CLI

```sh
pnpm cli:build
```

本機建置產物不包含 MangoDisk 正式發布流程提供的簽名、公證和更新元資料，僅用於開發與驗證。

## 參與貢獻

歡迎提交問題、清理規則、修復和新功能。開始前請閱讀
[`CONTRIBUTING.md`](CONTRIBUTING.md) 和 [`AGENTS.md`](AGENTS.md)。

一般清理規則應使用經過建置期驗證的宣告式 TOML。規則結構、安全限制和驗證方式請參閱
[`src-tauri/crates/mangodisk-core/rules/README.md`](src-tauri/crates/mangodisk-core/rules/README.md)。

提交修改前，請至少執行：

```sh
pnpm check
cargo test --manifest-path src-tauri/Cargo.toml -p mangodisk-core
```

發現安全問題時，請按照 [`SECURITY.md`](SECURITY.md) 透過 GitHub Security Advisories 私下報告，不要建立公開 Issue。

## 技術架構

- [Tauri 2](https://tauri.app/)：桌面執行時與系統整合
- [Rust](https://www.rust-lang.org/)：掃描、檔案系統、安全驗證和清理執行
- [Vue 3](https://vuejs.org/) 與 [TypeScript](https://www.typescriptlang.org/)：桌面使用者介面

## 授權條款

MangoDisk 採用 [GNU General Public License v3.0](https://github.com/harry0703/MangoDisk/blob/main/LICENSE) 開放原始碼。第三方元件仍適用各自的授權條款。
