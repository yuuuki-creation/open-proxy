#!/bin/sh
# op-agent 安装脚本，由主控生成（版本号和二进制的 SHA-256 在下载时填好）。
# 用法：curl -fsSL <主控地址>/api/agent/install.sh | bash -s -- <主控地址> <Token>
# 装好后 Agent 作为 systemd 服务 op-agent 运行；文件位置见 main 分支 architecture.md「发布、安装与升级」。
# 测试时可以用环境变量 OP_AGENT_SLICE 指定 systemd slice（测试 VPS 上所有进程都要在 open-proxy.slice 里）。
set -eu

VERSION="__VERSION__"
SHA256_AMD64="__SHA256_AMD64__"
SHA256_ARM64="__SHA256_ARM64__"

MASTER_URL="${1:-}"
TOKEN="${2:-}"

fail() {
    echo "安装失败：$*" >&2
    exit 1
}

[ -n "$MASTER_URL" ] && [ -n "$TOKEN" ] || fail "用法：bash install.sh <主控地址> <Token>"
[ "$(id -u)" = "0" ] || fail "要用 root 运行"
command -v systemctl >/dev/null 2>&1 || fail "需要 systemd"
command -v curl >/dev/null 2>&1 || fail "需要 curl"

case "$(uname -m)" in
    x86_64 | amd64) ARCH=amd64; WANT="$SHA256_AMD64" ;;
    aarch64 | arm64) ARCH=arm64; WANT="$SHA256_ARM64" ;;
    *) fail "不支持的架构 $(uname -m)，只支持 amd64 和 arm64" ;;
esac
[ -n "$WANT" ] || fail "主控没有 $ARCH 架构的 Agent 二进制"

MASTER_URL="${MASTER_URL%/}"
echo "安装 op-agent $VERSION（$ARCH），主控 $MASTER_URL"

TMP="$(mktemp /usr/local/bin/.op-agent.XXXXXX)"
trap 'rm -f "$TMP"' EXIT
# Token 放在请求头里，不放 URL
curl -fsSL -H "Authorization: Bearer $TOKEN" -o "$TMP" "$MASTER_URL/api/agent/binary/$VERSION/$ARCH" ||
    fail "下载 Agent 失败（Token 是否已失效？）"
GOT="$(sha256sum "$TMP" | cut -d' ' -f1)"
[ "$GOT" = "$WANT" ] || fail "下载的文件校验不对（期望 $WANT，实际 $GOT）"
chmod 755 "$TMP"

# 重装时先停掉旧的
systemctl stop op-agent 2>/dev/null || true
mv -f "$TMP" /usr/local/bin/op-agent
trap - EXIT

mkdir -p /etc/op-agent /var/lib/op-agent
umask 077
printf 'MASTER_URL=%s\nTOKEN=%s\n' "$MASTER_URL" "$TOKEN" > /etc/op-agent/op-agent.conf
chmod 600 /etc/op-agent/op-agent.conf

SLICE_LINE=""
if [ -n "${OP_AGENT_SLICE:-}" ]; then
    SLICE_LINE="Slice=$OP_AGENT_SLICE"
fi
cat > /etc/systemd/system/op-agent.service <<UNIT
[Unit]
Description=open-proxy Agent
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=/usr/local/bin/op-agent
Restart=always
RestartSec=5
LimitNOFILE=1048576
$SLICE_LINE

[Install]
WantedBy=multi-user.target
UNIT

systemctl daemon-reload
systemctl enable --now op-agent >/dev/null 2>&1 || fail "启动 op-agent 服务失败，看 journalctl -u op-agent"
sleep 2
if systemctl is-active --quiet op-agent; then
    echo "op-agent 已安装并启动。查看日志：journalctl -u op-agent -f"
else
    fail "op-agent 没有跑起来，看 journalctl -u op-agent"
fi
