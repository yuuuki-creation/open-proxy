#!/bin/bash
# M6 端口跳跃（nftables）的运行验证（调试记录 R20260927-agent-03）。在测试 VPS 上以 root 执行：
#   bash /opt/open-proxy/src/agent/agent/test/scripts/m6-firewall.sh <run-id> <开发电脑名>
# 前提：/opt/open-proxy/src/agent 已切到要测的提交；读规则要用 nft 命令（没有就装 nftables 包）。
# Agent 用测试专用的表 inet open_proxy_test（test-vps.md「安全规则」）；只用本机回环地址、21000–21999 和
# 30000–30999 端口；所有进程在 open-proxy.slice 里；结束时停掉本轮的进程、删表、删测试锁。
set -u
RUN=${1:?用法: m6-firewall.sh <run-id> <开发电脑名>}
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

if ! command -v nft >/dev/null; then
  echo "== 安装 nftables（只要 nft 命令读规则）"
  DEBIAN_FRONTEND=noninteractive apt-get install -y -q nftables </dev/null >/dev/null || exit 1
fi
echo "nftables 服务：$(systemctl is-enabled nftables 2>&1) / $(systemctl is-active nftables 2>&1)（应该是 disabled / inactive）"
echo "== 测试前的表"
nft list tables

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

rnd() { od -An -tx1 -N16 /dev/urandom | tr -d ' \n'; }
TOKEN=$(rnd)
printf 'MASTER_URL=http://127.0.0.1:21000\nTOKEN=%s\n' $TOKEN > $D/op-agent.conf
printf 'MASTER_URL=http://127.0.0.1:21099\nTOKEN=%s\n' $TOKEN > $D/op-agent-nomaster.conf
chmod 600 $D/op-agent*.conf

hy() { echo "{\"id\":\"$1\",\"port\":$2,\"userIds\":[\"1\"],\"hysteria2\":{\"portHopping\":{\"start\":$3,\"end\":$4}}}"; }
users="\"users\":[{\"id\":\"1\",\"password\":\"$(rnd)\"}]"
# 节点 1、2 正常；3 范围倒了；4 和 2 重叠；5 盖住了别的节点的端口
echo "{$users,\"nodes\":[$(hy 1 21002 30000 30099),$(hy 2 21003 30100 30199),$(hy 3 21004 30300 30250),$(hy 4 21005 30150 30260),$(hy 5 21006 21000 21010)]}" > $D/stateA.json
# 节点 1 换范围，其他节点删掉
echo "{$users,\"nodes\":[$(hy 1 21002 30300 30399)]}" > $D/stateB.json
echo "{$users}" > $D/stateC.json

push() { curl -s --data-binary @$D/$1 http://127.0.0.1:21000/ctl/state; sleep 1; }
last_report() { grep stateReport $D/master.log | tail -1; }
show() { nft list table inet $TABLE 2>&1; }
stray() { nft add rule inet $TABLE port_hopping udp dport 30900 counter comment '"stray"'; }

start master $FM -listen 127.0.0.1:21000 -token $TOKEN -state $D/stateA.json
sleep 1
start agent $AGENT -config $D/op-agent.conf -data-dir $D/data -nft-table $TABLE
sleep 3

echo "== 1. 状态 A：节点 1、2 有规则，3、4、5 报端口跳跃失败"
last_report
show
echo "== 2. 手工加一条规则，再推一次同样的状态：整体重建，多出来的规则没了、没有重复"
stray
push stateA.json
show
echo "== 3. 状态 B：节点 1 换范围，其他节点删掉"
push stateB.json
last_report
show
echo "== 4. 手工加一条规则后重启 Agent：按本地保存的状态 B 重建"
stray
systemctl stop op-$RUN-agent
start agent2 $AGENT -config $D/op-agent.conf -data-dir $D/data -nft-table $TABLE
sleep 3
show
echo "== 5. 状态 C（没有节点）：删表"
push stateC.json
last_report
show
echo "== 6. 没有本地状态、连不上主控时启动：清掉以前留下的表"
systemctl stop op-$RUN-agent2
rm -f $D/data/state.pb
nft add table inet $TABLE
nft add chain inet $TABLE port_hopping '{ type nat hook prerouting priority dstnat; }'
stray
show | head -3
start agent3 $AGENT -config $D/op-agent-nomaster.conf -data-dir $D/data -nft-table $TABLE
sleep 2
show
systemctl stop op-$RUN-agent3

echo "== 测试后的表（应该和测试前一样）"
nft list tables
echo "== Agent 日志（警告以上，含 sing-box 自己的日志）"
cat $D/agent.log $D/agent2.log $D/agent3.log | grep -vE 'level=(INFO|DEBUG)' | tail -20
