#!/bin/sh
# Agent 二进制的签名工具（architecture.md「签名」）：Ed25519，对二进制的全部内容签名，
# 签名是 64 字节原始数据，写到 <二进制>.sig；主控把它随 Upgrade 下发，Agent 用内置公钥验证。
#
# 用法：
#   sign-agent.sh genkey <私钥.pem>         生成私钥（离线保管，不要放到主控上）
#   sign-agent.sh pubkey <私钥.pem>         打印公钥（base64），编译 Agent 时注入：
#                                          -ldflags "-X main.upgradePublicKey=<公钥>"
#   sign-agent.sh sign <私钥.pem> <文件>...  给文件签名
set -eu

cmd="${1:-}"
case "$cmd" in
    genkey)
        [ $# -eq 2 ] || { echo "用法：$0 genkey <私钥.pem>" >&2; exit 1; }
        umask 077
        openssl genpkey -algorithm ed25519 -out "$2"
        ;;
    pubkey)
        [ $# -eq 2 ] || { echo "用法：$0 pubkey <私钥.pem>" >&2; exit 1; }
        # DER 编码的公钥最后 32 字节就是原始公钥
        openssl pkey -in "$2" -pubout -outform DER | tail -c 32 | base64
        ;;
    sign)
        [ $# -ge 3 ] || { echo "用法：$0 sign <私钥.pem> <文件>..." >&2; exit 1; }
        key="$2"
        shift 2
        for f in "$@"; do
            openssl pkeyutl -sign -inkey "$key" -rawin -in "$f" -out "$f.sig"
            echo "已签名 $f"
        done
        ;;
    *)
        echo "用法：$0 genkey|pubkey|sign ..." >&2
        exit 1
        ;;
esac
