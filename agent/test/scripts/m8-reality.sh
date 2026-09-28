#!/bin/bash
# M8 REALITY 目标检测与扫描的运行验证（调试记录 R20260927-agent-06）。在测试 VPS 上以 root 执行：
#   bash /opt/open-proxy/src/agent/agent/test/scripts/m8-reality.sh <run-id> <开发电脑名>
# 前提：/opt/open-proxy/src/agent 已切到要测的提交。
# 检测只访问几个公开的 HTTPS 网站（几次正常访问，不算扫描）。按 test-vps.md 不对外扫描网段：扫描只扫本机回环网段，
# 其中 127.0.0.2:443 用测试专用表 inet open_proxy_test 里的一条 output 规则转到本机的测试 TLS 服务（不占用 443）。
# 只用 21000–21999 端口；所有进程在 open-proxy.slice 里；结束时停掉本轮的进程、删表、删测试锁。
set -u
RUN=${1:?用法: m8-reality.sh <run-id> <开发电脑名>}
PC=${2:-unknown}
ROOT=/opt/open-proxy
D=$ROOT/runs/$RUN
LOCK=$ROOT/LOCK
TABLE=open_proxy_test

if [ -e $LOCK ]; then
  echo "测试锁被占用：$(cat $LOCK)"
  exit 1
fi
echo "$RUN agent $PC $(date -Is)" > $LOCK
cleanup() {
  systemctl stop "op-$RUN-*" 2>/dev/null
  systemctl reset-failed "op-$RUN-*" 2>/dev/null
  nft delete table inet $TABLE 2>/dev/null
  rm -f $LOCK
}
trap cleanup EXIT

mkdir -p $D/data
. $ROOT/env.sh
cd $ROOT/src/agent/agent || exit 1
H=$(git rev-parse --short HEAD </dev/null)
echo "提交 $H，结果目录 $D"

run() { systemd-run --quiet --scope --slice=open-proxy.slice -- "$@" </dev/null; }
start() {
  local name=$1
  shift
  systemd-run --quiet --unit=op-$RUN-$name --slice=open-proxy.slice \
    -p StandardOutput=file:$D/$name.log -p StandardError=file:$D/$name.log "$@"
}

for p in cmd/op-agent test/fakemaster; do
  run go build -p 6 -tags with_quic,with_utls -trimpath -o $ROOT/bin/$(basename $p)-agent-$H ./$p || exit 1
done
AGENT=$ROOT/bin/op-agent-agent-$H
FM=$ROOT/bin/fakemaster-agent-$H

TOKEN=$(od -An -tx1 -N16 /dev/urandom | tr -d ' \n')
printf 'MASTER_URL=http://127.0.0.1:21000\nTOKEN=%s\n' $TOKEN > $D/op-agent.conf
chmod 600 $D/op-agent.conf

start master $FM -listen 127.0.0.1:21000 -token $TOKEN -tls-listen 127.0.0.1:21443 -tls-name scan-test.example.com
sleep 1
start agent $AGENT -config $D/op-agent.conf -data-dir $D/data -nft-table $TABLE
sleep 3

req() { curl -s -X POST -d "$1" "http://127.0.0.1:21000/ctl/request?timeout=${2:-70s}"; }
scan() { req "{\"scanRealityTargets\":{\"cidr\":\"$1\",\"concurrency\":${2:-0},\"maxPerSecond\":${3:-0}}}" 5m; }

echo "== 1. 检测：几个公开网站、本机的自签 TLS 服务、连不上的、解析不了的、格式不对的"
req '{"checkRealityTargets":{"targets":["www.microsoft.com","www.apple.com:443","example.com","127.0.0.1:21443","127.0.0.1:21998","no-such-host.invalid","bad:port:x"]}}'

echo "== 2. 扫描本机回环网段 127.0.0.0/29（并发 2，每秒 4 个）：只有 127.0.0.2 合格"
nft add table inet $TABLE
nft add chain inet $TABLE output '{ type nat hook output priority -100; }'
nft add rule inet $TABLE output ip daddr 127.0.0.2 tcp dport 443 redirect to :21443
begin=$(date +%s%3N)
scan 127.0.0.0/29 2 4
echo "耗时 $(($(date +%s%3N) - begin)) 毫秒（8 个地址、每秒 4 个，约 2 秒）"

echo "== 3. 扫描参数不对"
scan 10.0.0.0/8
scan ::1/128
scan 127.0.0.0/20 0 1

echo "== 4. 同时发两个扫描：第二个被拒绝"
scan 127.0.0.0/26 4 4 > $D/scan-long.txt &
sleep 2
scan 127.0.0.0/30
wait
cat $D/scan-long.txt

echo "== Agent 日志"
grep -E '扫描|REALITY' $D/agent.log | cut -c1-200
grep -vE 'level=(INFO|DEBUG)' $D/agent.log | tail -10 | cut -c1-200
