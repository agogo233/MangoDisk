<h1 align="center">
  <img src="public/mangodisk.svg" width="40" alt="MangoDisk アプリアイコン"> MangoDisk
</h1>

<p align="center">
<a href="README.md">English</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.zh-TW.md">繁體中文</a> · 日本語
</p>

<p align="center">
<a href="https://github.com/harry0703/MangoDisk/releases/latest"><img alt="最新リリース" src="https://img.shields.io/github/v/release/harry0703/MangoDisk?display_name=tag&sort=semver"></a>
  <img alt="macOS 対応" src="https://img.shields.io/badge/macOS-supported-111827?logo=apple&logoColor=white">
  <img alt="Windows 対応" src="https://img.shields.io/badge/Windows-supported-2563eb?logo=windows&logoColor=white">
  <img alt="Tauri 2" src="https://img.shields.io/badge/Tauri-2-24c8db?logo=tauri&logoColor=white">
  <img alt="Rust Core" src="https://img.shields.io/badge/core-Rust-b7410e?logo=rust&logoColor=white">
</p>

<p align="center">
  <a href="https://mangodisk.app/ja">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/readme/ja-dark.jpg">
      <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/readme/ja-light.jpg">
      <img src="https://assets.mangodisk.app/images/readme/ja-light.jpg" width="1200" alt="MangoDisk でディスクをすっきり整理し、空き容量を増やす">
    </picture>
  </a>
</p>

## MangoDisk でできること

### ディープクリーン

ディープクリーンは MangoDisk の中心となる機能です。OS、アプリ、開発環境、ローカルプロジェクトが使用している領域をスキャンし、削除できるデータと解放可能な容量をカテゴリ別にまとめて表示します。

- **システムとアプリ**：システムとユーザーの一時ファイル、アプリキャッシュ、ブラウザキャッシュ、診断ログ、アンインストール後に残ったファイル。
- **開発ツール**：パッケージマネージャーのダウンロードキャッシュ、IDE のインデックス、開発ツールの一時ファイル、Xcode や Windows の開発環境が生成したデータ。
- **プロジェクトのビルド成果物**：Node.js の依存関係やフレームワークキャッシュ、Rust の `target` ディレクトリ、Gradle、Swift、Python、.NET、Godot、CMake などが生成する再作成可能なデータ。
- **AI モデルとキャッシュ**：ダウンロード済みのローカル AI モデルと一時転送ファイル。容量の大きいモデルデータを見つけやすくします。
- **アプリ容量の最適化**：現在のデバイスでは使われないプロセッサ向けコードを取り除き、対応アプリの容量を減らします。

スキャンではファイル情報を読み取るだけで、内容を自動的に削除することはありません。結果にはカテゴリ、サイズ、場所、リスクが表示されます。スマート選択を使うことも、項目を一つずつ確認して選ぶこともできます。解放できる容量を確認してからクリーンアップを実行できます。

### 大容量ファイル

ディスクまたは選択したフォルダーをスキャンし、動画、オーディオ、画像、アーカイブ、書類、インストーラーなど、種類別に大容量ファイルを確認できます。最小ファイルサイズの指定や使用容量順の並べ替えに対応し、削除前に保存場所を開いて内容を確認できます。

### 重複ファイル

選択したフォルダーをスキャンし、ファイル名ではなく内容を比較して完全に同一のファイルを見つけます。結果はグループごとにコピー数、1 ファイルあたりのサイズ、最大解放可能容量を表示します。スマート選択を使っても、各グループに少なくとも 1 ファイルは残ります。

### アプリのアンインストールとクリーンアップ

インストール済みアプリのサイズ、状態、関連ファイルを確認できます。アンインストール時には、キャッシュ、設定、アプリのプライベートデータを個別に確認できます。MangoDisk は、アンインストールに必要な項目、再作成可能なデータ、ユーザーファイルを含む可能性のある項目を区別し、アプリが実行中または保護されている場合は警告します。

### ディスク容量分析

ディスクまたは選択したフォルダーをツリーマップとリストで分析し、フォルダー、ファイル数、使用容量を表示します。フォルダー階層をたどって容量の大きいディレクトリやファイルを見つけ、保存場所を直接開くことができます。

## クリーンアップルールと安全性

MangoDisk は、サードパーティ製プロジェクトのルールをそのままコピーせず、独自のクロスプラットフォーム対応クリーンアップルールを管理しています。Windows では Winapp2.ini、macOS では関連するオープンソースプロジェクトを調査の手がかりとして参照する場合がありますが、それだけを根拠に削除対象を決めることはありません。

候補となるルールは、リリースに含める前に次の項目を確認します。

- **公式情報の確認**：Microsoft、Apple、またはソフトウェアベンダーの資料を使い、パスの用途とデータの所有者を確認します。
- **安全な削除範囲の定義**：安全に再作成できるデータだけを対象とし、個人ファイル、アプリのプライベートデータ、保護されたシステムパスを除外します。
- **実機での検証**：対象となる Windows または macOS 環境で、パス、クリーンアップ結果、エラー時の挙動をテストします。

情報源の確認、安全性レビュー、実機テストをすべて通過したルールだけが製品版に追加されます。つまり、**サードパーティ製プロジェクトは調査の手がかりであり、採用の可否は公式情報と実機検証に基づいて判断します。**

ルールライブラリはすべて公開されているため、各ルールと変更履歴を確認できます：[MangoDisk のクリーンアップルールを見る](https://github.com/harry0703/MangoDisk/tree/main/src-tauri/crates/mangodisk-core/rules)。

MangoDisk は、解放できる容量よりもデータの安全性を優先します。安全な範囲を明確に確認できないデータは製品版のルールに含めません。また、削除前に対象を確認し、必要な項目だけを選択できます。

## スクリーンショット

<p align="center">
<strong>ディープクリーン</strong><br>
<sub>システム、アプリ、開発ツール、プロジェクトから削除可能なデータをスキャンし、クリーンアップ前に確認できます。</sub>
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/screenshots/ja/dark-01-deep-cleanup.jpg">
    <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/screenshots/ja/light-01-deep-cleanup.jpg">
    <img src="https://assets.mangodisk.app/images/screenshots/ja/light-01-deep-cleanup.jpg" width="1200" alt="MangoDisk ディープクリーン画面">
  </picture>
</p>

<table>
  <tr>
    <td width="50%" align="center">
<strong>大容量ファイル</strong><br>
<sub>種類やサイズから大容量ファイルを見つけ、削除前に確認できます。</sub><br><br>
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/screenshots/ja/dark-02-large-file-cleanup.jpg">
        <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/screenshots/ja/light-02-large-file-cleanup.jpg">
        <img src="https://assets.mangodisk.app/images/screenshots/ja/light-02-large-file-cleanup.jpg" width="100%" alt="MangoDisk 大容量ファイル画面">
      </picture>
    </td>
    <td width="50%" align="center">
<strong>重複ファイル</strong><br>
<sub>ファイルの内容を比較して完全な重複を見つけ、各グループに少なくとも 1 つを残します。</sub><br><br>
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/screenshots/ja/dark-03-duplicate-cleanup.jpg">
        <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/screenshots/ja/light-03-duplicate-cleanup.jpg">
        <img src="https://assets.mangodisk.app/images/screenshots/ja/light-03-duplicate-cleanup.jpg" width="100%" alt="MangoDisk 重複ファイル画面">
      </picture>
    </td>
  </tr>
  <tr>
    <td width="50%" align="center">
<strong>アプリのアンインストールとクリーンアップ</strong><br>
<sub>アプリをアンインストールし、キャッシュ、設定、プライベートデータを確認できます。</sub><br><br>
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/screenshots/ja/dark-04-app-uninstaller.jpg">
        <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/screenshots/ja/light-04-app-uninstaller.jpg">
        <img src="https://assets.mangodisk.app/images/screenshots/ja/light-04-app-uninstaller.jpg" width="100%" alt="MangoDisk アプリアンインストーラー画面">
      </picture>
    </td>
    <td width="50%" align="center">
<strong>ディスク容量分析</strong><br>
<sub>ツリーマップとリストから、容量を多く使用しているデータを見つけます。</sub><br><br>
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="https://assets.mangodisk.app/images/screenshots/ja/dark-05-disk-space-analysis.jpg">
        <source media="(prefers-color-scheme: light)" srcset="https://assets.mangodisk.app/images/screenshots/ja/light-05-disk-space-analysis.jpg">
        <img src="https://assets.mangodisk.app/images/screenshots/ja/light-05-disk-space-analysis.jpg" width="100%" alt="MangoDisk ディスク容量分析画面">
      </picture>
    </td>
  </tr>
</table>

## インストールと実行

Homebrew を使って macOS に MangoDisk をインストールできます。

```sh
brew install --cask harry0703/tap/mangodisk
```

または、[MangoDisk 公式サイト](https://mangodisk.app/ja) か [GitHub Releases](https://github.com/harry0703/MangoDisk/releases/latest) から最新版をダウンロードできます。

- **macOS**：DMG を開き、MangoDisk を「アプリケーション」フォルダーへドラッグします。
- **Windows**：Windows インストーラーを実行し、画面の案内に従います。

> [!IMPORTANT]
> クリーンアップとアンインストールではファイルが削除されます。個人ファイルを削除する前に選択内容を確認し、重要なデータは信頼できる場所にバックアップしてください。

## CLI クイックスタート

Homebrew を使って macOS にスタンドアロン版 CLI をインストールできます。

```sh
brew install harry0703/tap/mangodisk-cli
```

Windows では WinGet でインストールできます。

```powershell
winget install --id MangoDisk.CLI -e
```

どちらの方法でも `mangodisk` がコマンドパスに追加されます。コマンドがすぐに見つからない場合は、新しいターミナルを開いてバージョンを確認してください。

```sh
mangodisk --version
```

CLI はデスクトップアプリと同じ、安全性を重視したクリーンアップエンジンを使用します。

```sh
# 変更を加えず、削除可能な内容をスキャンして表示
mangodisk clean

# デスクトップアプリと同じスマート選択を適用
mangodisk clean --apply

# ファイルを削除せず、選択可能な内容をすべてプレビュー
mangodisk clean --apply --selection all --dry-run

# 機械処理しやすい JSON 形式で出力
mangodisk clean --format json --no-progress
```

既定ではスキャンだけを行います。非対話環境でクリーンアップを実行する場合は、明示的な確認オプションも必要です。利用できるすべてのオプションは次のコマンドで確認できます。

```sh
mangodisk clean --help
```

## ソースからビルド

### 前提条件

- Node.js 24 LTS
- pnpm 11.13.1
- 安定版 Rust
- macOS：Xcode Command Line Tools
- Windows：Visual Studio 2022 Build Tools（**C++ によるデスクトップ開発**を含む）
- Windows：Microsoft Edge WebView2 Runtime

詳細なプラットフォーム要件については、[Tauri 2 の前提条件](https://v2.tauri.app/start/prerequisites/) を参照してください。

### ソースを取得してデスクトップアプリを実行

```sh
git clone https://github.com/harry0703/MangoDisk.git
cd MangoDisk
pnpm install --frozen-lockfile
pnpm tauri:dev
```

### 必要なチェックを実行

```sh
pnpm check
cargo test --manifest-path src-tauri/Cargo.toml -p mangodisk-core
```

### デスクトップインストーラーをビルド

```sh
pnpm tauri:build
```

### CLI をビルド

```sh
pnpm cli:build
```

ローカルビルドには、MangoDisk の公式リリースで提供される署名、公証、アップデート用メタデータは含まれません。開発と検証にのみ使用してください。

## 貢献

不具合報告、クリーンアップルール、修正、新機能の提案を歓迎します。作業を始める前に [`CONTRIBUTING.md`](CONTRIBUTING.md) と [`AGENTS.md`](AGENTS.md) をお読みください。

通常のクリーンアップ対象は、ビルド時に検証される宣言的な TOML ルールとして追加してください。ルールスキーマ、セーフティ制約、検証手順については [`src-tauri/crates/mangodisk-core/rules/README.md`](src-tauri/crates/mangodisk-core/rules/README.md) を参照してください。

変更を提出する前に、少なくとも次を実行してください:

```sh
pnpm check
cargo test --manifest-path src-tauri/Cargo.toml -p mangodisk-core
```

セキュリティ上の問題は、[`SECURITY.md`](SECURITY.md) の案内に従って GitHub Security Advisories から非公開で報告してください。公開 Issue には投稿しないでください。

## 技術スタック

- [Tauri 2](https://tauri.app/): デスクトップランタイムおよびシステム統合
- [Rust](https://www.rust-lang.org/): スキャン、ファイルシステムアクセス、安全性の検証、クリーンアップ実行
- [Vue 3](https://vuejs.org/) および [TypeScript](https://www.typescriptlang.org/): デスクトップユーザーインターフェース

## ライセンス

MangoDisk は [GNU General Public License v3.0](https://github.com/harry0703/MangoDisk/blob/main/LICENSE) に基づくオープンソースソフトウェアです。サードパーティ製コンポーネントには、それぞれのライセンスが適用されます。
