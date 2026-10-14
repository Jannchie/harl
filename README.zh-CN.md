# harl

[English](README.md) | 简体中文 | [日本語](README.ja.md)

harl 把 AI 编程 agent（Claude Code、Codex、preferred-harness（pfh）、dsh）的会话日志归档到本地只追加、按内容寻址的存储中，并提供 Web 仪表盘，用于浏览对话、分析 token 用量、费用与耗时。

各 harness 会轮转、压缩或删除自己的日志；harl 保留每个版本的逐字节副本。每次 capture 只存储未见过的行：520 MiB 的日志目录归档后约 150 MiB，之后每次增量通常只有几 KiB。所有 harness 被归一化为同一种事件流，仪表盘因此可以按相同口径比较。

## 安装

```sh
# Windows（PowerShell）
irm https://github.com/Jannchie/harl/releases/latest/download/install.ps1 | iex
# macOS / Linux
curl -fsSL https://github.com/Jannchie/harl/releases/latest/download/install.sh | sh
```

安装脚本会校验 sha256，并安装到 `~/.harl/bin`。

## 快速上手

```sh
harl service install    # 登录后自动运行 harl serve，进程退出时自动重启
```

打开 http://127.0.0.1:7461。服务启动时先 capture 已有会话，之后监听会话目录并增量 capture。如需在前台运行，使用 `harl serve --open`。

运行中的服务会自动安装新版本。在 `~/.harl/config.toml` 中设置 `[update] auto = false` 可改为仅提示，也可以手动执行 `harl update`。

## 常用命令

```sh
harl capture                 # 对所有会话文件做一次快照
harl status                  # 存储总览与压缩比
harl events <路径片段>        # 以 canonical 事件流（NDJSON）输出一个会话
harl check --all             # 还原每个快照并逐字节校验
harl --help                  # 完整命令列表
```

Web 界面与命令行均支持英文、简体中文和日文。Web 界面跟随浏览器语言，侧栏中可切换；命令行依次读取 `HARL_LANG`、`LC_ALL` / `LANG` 和系统语言。

## 多台机器

harl 通过任意 S3 兼容存储（S3、Cloudflare R2、MinIO、B2）或共享目录同步，不需要服务器。每台机器推送自己的数据，运行仪表盘的机器拉取其他机器的数据。在 `~/.harl/config.toml` 中配置远端：

```toml
[remote]
url = "s3://my-bucket/harl"        # 或目录：url = "D:/nas/harl"
pull = true                        # 仅采集的机器设为 false

[remote.options]                   # 交给 object_store；未填写的 aws_* 从同名环境变量读取
aws_endpoint = "https://<account>.r2.cloudflarestorage.com"
aws_region = "auto"
```

仅负责采集的机器执行 `harl service install --agent`：只采集与同步，不启动 Web 界面。双向同步均为增量；`harl sync` 可手动同步一次。

开机自启均为用户级：Windows 通过 HKCU Run 启动无窗口的 `harlw.exe`（日志位于 `~/.harl/logs/harl.log`），macOS 使用 launchd agent，Linux 使用 systemd 用户服务（服务器上无登录会话时需执行一次 `sudo loginctl enable-linger $USER`）。

## 从源码构建

```sh
pnpm install && pnpm build:all   # 构建 web/dist，再构建嵌入界面的 release 二进制
pnpm dev                         # Vite 监听 127.0.0.1:7462，将 /api 代理到 127.0.0.1:7461 的 harl serve
```

存储格式、同步协议、统计口径与 JSON API 见 [docs/architecture.md](docs/architecture.md)。

## License

MIT
