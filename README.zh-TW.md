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

深度清理是 MangoDisk 的核心功能。它會集中掃描系統、應用程式、開發工具和本機專案中的快取、暫存檔案及可重建內容，並依類別彙整可釋放空間：

- **系統與使用者快取**：清理系統暫存檔案、診斷資料，以及儲存在使用者目錄中的可重建快取。
- **應用程式快取**：清理常用應用程式執行時產生的快取、記錄檔、更新套件和暫存內容。
- **瀏覽器資料**：清理 Chrome、Edge、Firefox、Brave、Arc、Opera 等瀏覽器產生的快取和暫存網頁資料。
- **開發工具與 Xcode**：清理套件管理工具下載快取、IDE 索引、編譯快取，以及 Xcode 產生的裝置支援、封存和開發資料。
- **容器快取**：清理 Docker 等容器工具產生的閒置建置快取和可重新產生的暫存資料。
- **專案建置產物**：識別 Node.js、Rust、Gradle、Swift、Python、.NET、Godot、CMake 等專案中可重新產生的相依套件、快取和建置目錄。
- **AI 模型與快取**：識別本機 AI 模型、下載快取和暫存傳輸檔案，協助找出佔用空間較大的模型資料。
- **應用程式最佳化**：清理支援的應用程式中目前裝置用不到的處理器程式碼，在不影響正常使用的情況下減少空間佔用。

掃描過程只讀取檔案資訊，不會自動刪除任何內容。你可以採用智慧推薦，也可以逐項確認，查看預估可釋放空間後再執行清理。

### 大型檔案清理

快速找出磁碟或指定資料夾中佔用空間較大的檔案，並依類型和大小查看。確認檔案內容和位置後，再決定是否刪除。

### 重複檔案清理

透過檔案內容識別完全相同的副本，而不只比較檔名。結果會依群組顯示副本數量、單一檔案大小和最多可釋放空間；智慧選取會為每組保留至少一份檔案。

### 解除安裝應用程式與殘留清理

查看已安裝應用程式的大小、執行狀態和相關檔案。解除安裝前可一併檢查快取、設定和殘留資料，並區分可重新產生的內容與可能包含個人檔案的資料；如果應用程式正在執行或受系統保護，MangoDisk 會提前提示。

### 啟動項目管理

查看和管理 macOS 與 Windows 中隨系統自動執行的程式。關閉不需要的啟動項目，有助於縮短開機或登入等待時間、減少背景資源占用；需要時也可以隨時重新啟用。

### 磁碟空間分析

透過樹狀圖和清單查看磁碟或指定資料夾的空間分布，逐層找出佔用最多的目錄和檔案，並直接開啟其所在位置。

### 操作紀錄

回顧清理、檔案刪除、應用程式解除安裝和啟動項目調整紀錄，查看處理結果與釋放空間，方便確認每次操作。

## 安全設計與清理規則

MangoDisk 預設以唯讀方式掃描。清理、永久刪除、解除安裝應用程式或修改啟動項目前，會顯示操作影響並要求確認；操作完成後，可以在操作紀錄中查看處理結果。

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
> 清理、永久刪除和解除安裝操作可能無法復原。請在執行前確認內容，並為重要資料保留可靠的備份；修改啟動項目前也請確認相關程式的用途。

## CLI 快速上手

macOS 使用者可以透過 Homebrew 安裝獨立 CLI：

```sh
brew install harry0703/tap/mangodisk-cli
```

Homebrew 會將 `mangodisk` 加入命令路徑。如果安裝後暫時找不到命令，請重新開啟終端機，再檢查版本：

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

`mangodisk clean` 預設只會掃描，不會修改檔案。在非互動式環境執行實際清理時，還必須傳入 `--yes` 明確確認；完整選項請執行：

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
