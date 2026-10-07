#!/bin/sh
# 安装 / 升级 harl（macOS、Linux）。
#
#   curl -fsSL https://github.com/Jannchie/harl/releases/latest/download/install.sh | sh
#
# 装完顺便设成开机自启：... | sh -s -- --service（看板）或 --agent（只采集，远程机器用）
# 其他：--version 0.2.0  --dir ~/.local/bin
set -eu

repo=Jannchie/harl
version=latest
dir="$HOME/.harl/bin"
service=""

while [ $# -gt 0 ]; do
  case "$1" in
    --service) service=serve ;;
    --agent) service=agent ;;
    --version) version="${2#v}"; shift ;;
    --dir) dir="$2"; shift ;;
    *) echo "未知参数：$1" >&2; exit 2 ;;
  esac
  shift
done

case "$(uname -s)" in
  Darwin) os=apple-darwin ;;
  Linux) os=unknown-linux-gnu ;;
  *) echo "不支持的系统：$(uname -s)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch=x86_64 ;;
  arm64 | aarch64) arch=aarch64 ;;
  *) echo "不支持的架构：$(uname -m)" >&2; exit 1 ;;
esac
asset="harl-$arch-$os.tar.gz"
if [ "$version" = latest ]; then
  base="https://github.com/$repo/releases/latest/download"
else
  base="https://github.com/$repo/releases/download/v$version"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
echo "下载 $base/$asset"
curl -fsSL "$base/$asset" -o "$tmp/$asset"
curl -fsSL "$base/$asset.sha256" -o "$tmp/$asset.sha256"
want=$(cut -d' ' -f1 < "$tmp/$asset.sha256")
if command -v sha256sum >/dev/null 2>&1; then got=$(sha256sum "$tmp/$asset" | cut -d' ' -f1); else got=$(shasum -a 256 "$tmp/$asset" | cut -d' ' -f1); fi
[ "$want" = "$got" ] || { echo "sha256 不一致：$got ≠ $want" >&2; exit 1; }
tar -xzf "$tmp/$asset" -C "$tmp"

mkdir -p "$dir"
for b in harl harlw; do
  src=$(find "$tmp" -type f -name "$b" | head -n 1)
  [ -n "$src" ] || { echo "包里没有 $b" >&2; exit 1; }
  # 先放临时名再 rename：正在运行的旧版本不受影响
  cp "$src" "$dir/.$b.new"
  chmod 755 "$dir/.$b.new"
  mv -f "$dir/.$b.new" "$dir/$b"
done

"$dir/harl" --version
case ":$PATH:" in
  *":$dir:"*) ;;
  *) echo "把 $dir 加进 PATH，例如在 ~/.profile 里加：export PATH=\"$dir:\$PATH\"" ;;
esac

if [ -n "$service" ]; then
  if [ "$service" = agent ]; then "$dir/harl" service install --agent; else "$dir/harl" service install; fi
elif "$dir/harl" service status 2>/dev/null | grep -q '运行中：是'; then
  # 已经装了开机自启：重启换成新版本
  "$dir/harl" service stop && "$dir/harl" service start
else
  echo "开机自启：harl service install（看板）或 harl service install --agent（只采集）"
fi
