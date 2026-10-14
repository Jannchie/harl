# harl

[English](README.md) | [简体中文](README.zh-CN.md) | 日本語

harl は AI コーディングエージェント（Claude Code、Codex、preferred-harness（pfh）、dsh）のセッションログを、ローカルの追記専用・コンテンツアドレス方式のストアにアーカイブし、会話の閲覧とトークン使用量・費用・所要時間の分析を行う Web ダッシュボードを提供する。

各ハーネスは自身のログをローテーション、圧縮、削除する。harl はすべてのバージョンをバイト単位で正確に保存する。capture ごとに未保存の行のみを格納するため、520 MiB のログディレクトリはアーカイブ後に約 150 MiB となり、以降の差分は通常数 KiB である。すべてのハーネスは共通のイベントストリームに正規化されるため、ダッシュボード上で同じ基準で比較できる。

## インストール

```sh
# Windows（PowerShell）
irm https://github.com/Jannchie/harl/releases/latest/download/install.ps1 | iex
# macOS / Linux
curl -fsSL https://github.com/Jannchie/harl/releases/latest/download/install.sh | sh
```

インストーラは sha256 を検証し、`~/.harl/bin` にインストールする。

## クイックスタート

```sh
harl service install    # ログイン時に harl serve を起動し、終了した場合は再起動する
```

http://127.0.0.1:7461 を開く。サーバーは起動時に既存のセッションを capture し、以降はセッションフォルダを監視して差分を capture する。フォアグラウンドで実行する場合は `harl serve --open` を使う。

実行中のサーバーは新しいバージョンを自動でインストールする。`~/.harl/config.toml` で `[update] auto = false` を設定すると通知のみになる。手動の場合は `harl update` を実行する。

## 主なコマンド

```sh
harl capture                 # すべてのセッションファイルを一度スナップショット
harl status                  # ストレージの概要と圧縮率
harl events <パスの一部>      # セッションを正規化イベント（NDJSON）として出力
harl check --all             # すべてのスナップショットを復元してバイト単位で検証
harl --help                  # コマンド一覧
```

Web UI と CLI は英語・簡体字中国語・日本語に対応する。Web UI はブラウザの言語に従い、サイドバーで切り替えられる。CLI は `HARL_LANG`、`LC_ALL` / `LANG`、システムの言語の順に参照する。

## 複数マシン

harl は S3 互換ストレージ（S3、Cloudflare R2、MinIO、B2）または共有フォルダを介して同期し、サーバーを必要としない。各マシンは自身のデータを push し、ダッシュボードを実行するマシンが他のマシンのデータを pull する。リモートは `~/.harl/config.toml` で設定する：

```toml
[remote]
url = "s3://my-bucket/harl"        # フォルダの場合：url = "D:/nas/harl"
pull = true                        # 収集専用のマシンでは false

[remote.options]                   # object_store に渡される。未設定の aws_* は同名の環境変数から読む
aws_endpoint = "https://<account>.r2.cloudflarestorage.com"
aws_region = "auto"
```

収集専用のマシンでは `harl service install --agent` を実行する。Web UI を起動せず、収集と同期のみを行う。同期は双方向とも差分で行われ、`harl sync` で手動実行もできる。

自動起動はユーザー単位で設定される。Windows は HKCU Run からウィンドウなしの `harlw.exe` を起動し（ログは `~/.harl/logs/harl.log`）、macOS は launchd agent、Linux は systemd ユーザーサービスを使う（ログインセッションのないサーバーでは `sudo loginctl enable-linger $USER` を一度実行する）。

## ソースからビルド

```sh
pnpm install && pnpm build:all   # web/dist をビルドし、UI を埋め込んだ release バイナリをビルド
pnpm dev                         # Vite（127.0.0.1:7462）が /api を 127.0.0.1:7461 の harl serve にプロキシ
```

ストレージ形式、同期プロトコル、統計の算出方法、JSON API は [docs/architecture.md](docs/architecture.md) を参照。

## ライセンス

MIT
