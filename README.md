# harl

English | [简体中文](README.zh-CN.md) | [日本語](README.ja.md)

harl archives the session logs of AI coding agents — Claude Code, Codex, preferred-harness (pfh) and dsh — into a local, append-only, content-addressed store, and serves a web dashboard for browsing conversations and analyzing token usage, cost and time.

Agent harnesses rotate, compact or delete their own logs; harl keeps a byte-exact copy of every version. Each capture stores only the lines it has not seen before, so a 520 MiB log directory archives to about 150 MiB and later captures cost a few KiB. All harnesses are normalized into one event stream, so the dashboard compares them on the same terms.

## Install

```sh
# Windows (PowerShell)
irm https://github.com/Jannchie/harl/releases/latest/download/install.ps1 | iex
# macOS / Linux
curl -fsSL https://github.com/Jannchie/harl/releases/latest/download/install.sh | sh
```

The installer verifies the sha256 checksum and installs to `~/.harl/bin`.

## Quick start

```sh
harl service install    # run harl serve at login and restart it if it exits
```

Open http://127.0.0.1:7461. The server captures existing sessions on startup, then watches the session folders and captures changes incrementally. To run it in the foreground instead, use `harl serve --open`.

New releases are installed automatically by the running server. Set `[update] auto = false` in `~/.harl/config.toml` to only show a notification, or run `harl update` manually.

## Usage

```sh
harl capture                 # snapshot all session files once
harl status                  # storage overview and compression ratio
harl events <path-part>      # print a session as canonical events (NDJSON)
harl check --all             # restore every snapshot and verify it byte for byte
harl --help                  # full command list
```

The web UI and the CLI are available in English, Simplified Chinese and Japanese. The web UI follows the browser language and has a switcher in the sidebar; the CLI follows `HARL_LANG`, then `LC_ALL` / `LANG`, then the system language.

## Multiple machines

harl syncs through any S3-compatible bucket (S3, Cloudflare R2, MinIO, B2) or a shared folder; no server is required. Each machine pushes its own data, and the machine running the dashboard pulls the others. Configure the remote in `~/.harl/config.toml`:

```toml
[remote]
url = "s3://my-bucket/harl"        # or a folder: url = "D:/nas/harl"
pull = true                        # set to false on capture-only machines

[remote.options]                   # passed to object_store; unset aws_* keys are read from environment variables
aws_endpoint = "https://<account>.r2.cloudflarestorage.com"
aws_region = "auto"
```

On a machine that only collects sessions, run `harl service install --agent`: it captures and syncs without starting the web UI. Sync is incremental in both directions; `harl sync` runs it once by hand.

Autostart is per user: a windowless `harlw.exe` launched from the HKCU Run key on Windows (logs in `~/.harl/logs/harl.log`), a launchd agent on macOS, and a systemd user service on Linux (run `sudo loginctl enable-linger $USER` on servers without a login session).

## Build from source

```sh
pnpm install && pnpm build:all   # builds web/dist, then the release binary with the UI embedded
pnpm dev                         # Vite on 127.0.0.1:7462, proxying /api to harl serve on 127.0.0.1:7461
```

Storage format, sync protocol, statistics methodology and the JSON API are described in [docs/architecture.md](docs/architecture.md).

## License

MIT
