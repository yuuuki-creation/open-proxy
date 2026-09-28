#!/bin/bash
# M7 升级、回滚、卸载的运行验证（调试记录 R20260927-agent-04）。在测试 VPS 上以 root 执行：
#   bash /opt/open-proxy/src/agent/agent/test/scripts/m7-upgrade.sh <run-id> <开发电脑名>
# 前提：/opt/open-proxy/src/agent 已切到要测的提交。要等试运行超时（3 分钟），整个脚本约 5 分钟。
# 模拟安装：二进制、配置、数据目录都放在结果目录的 inst/ 下；Agent 用 systemd-run 起成带 Restart=always 的临时服务。
# 只用本机回环地址和 21000–21999、30000–30999 端口；nftables 只用测试专用的表；结束时停掉本轮的进程、删表、删测试锁。
set -u
RUN=${1:?用法: m7-upgrade.sh <run-id> <开发电脑名>}
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

mkdir -p $D/dist
. $ROOT/env.sh
cd $ROOT/src/agent/agent || exit 1
H=$(git rev-parse --short HEAD </dev/null)
echo "提交 $H，结果目录 $D"

run() { systemd-run --quiet --scope --slice=open-proxy.slice -- "$@" </dev/null; }
start() {
  local name=$1
  shift
  systemd-run --quiet --unit=op-$RUN-$name --slice=open-proxy.slice \
    -p StandardOutput=append:$D/$name.log -p StandardError=append:$D/$name.log "$@"
}

run go build -p 6 -trimpath -o $ROOT/bin/fakemaster-agent-$H ./test/fakemaster || exit 1
FM=$ROOT/bin/fakemaster-agent-$H
eval "$(run $FM -keygen)" # 设置 PRIVATE、PUBLIC
V1=0.0.1-$H
V2=0.0.2-$H
V3=0.0.3-$H
build() { run go build -p 6 -tags with_quic,with_utls -trimpath -ldflags "-X main.version=$1 -X main.upgradePublicKey=$2" -o $3 ./cmd/op-agent; }
build $V1 "$PUBLIC" $D/dist/op-agent-$V1-amd64 || exit 1
build $V2 "$PUBLIC" $D/dist/op-agent-$V2-amd64 || exit 1
build $V3 "$PUBLIC" $D/dist/op-agent-$V3-amd64 || exit 1
build 0.0.1-nokey "" $D/dist/op-agent-nokey || exit 1

rnd() { od -An -tx1 -N16 /dev/urandom | tr -d ' \n'; }
TOKEN=$(rnd)
# 一个开了端口跳跃的 Hysteria2 节点，卸载时看 nftables 表有没有删掉
echo "{\"users\":[{\"id\":\"1\",\"password\":\"$(rnd)\"}],\"nodes\":[{\"id\":\"1\",\"port\":21002,\"userIds\":[\"1\"],\"hysteria2\":{\"portHopping\":{\"start\":30000,\"end\":30099}}}]}" > $D/state.json

install_agent() {
  mkdir -p $D/inst/bin $D/inst/etc $D/inst/data
  cp $1 $D/inst/bin/op-agent
  printf 'MASTER_URL=http://127.0.0.1:21000\nTOKEN=%s\n' $TOKEN > $D/inst/etc/op-agent.conf
  chmod 600 $D/inst/etc/op-agent.conf
}
start_agent() {
  start $1 -p Restart=always -p RestartSec=2 \
    $D/inst/bin/op-agent -config $D/inst/etc/op-agent.conf -data-dir $D/inst/data -nft-table $TABLE
}
upgrade() { curl -s -X POST "http://127.0.0.1:21000/ctl/upgrade?version=$1&arch=amd64${2:-}" | tr -d '\n '; echo; }
# 假主控收到的 Hello：版本和「上次升级回滚了」（去掉 Token）
hellos() { grep '"hello"' $D/$1.log | grep -oE '"(agentVersion|rolledBackFrom)": ?"[^"]*"' | paste -sd ' '; }
versions() { echo "二进制 $(run $D/inst/bin/op-agent -version 2>&1)，备份 $(run $D/inst/bin/op-agent.bak -version 2>&1)，升级标记 $(cat $D/inst/data/upgrade.json 2>&1)"; }
files() { find $D/inst 2>&1 | sort; }

start master $FM -listen 127.0.0.1:21000 -token $TOKEN -state $D/state.json -binary-dir $D/dist -sign-key $PRIVATE -reject-version $V3
sleep 1

echo "== 1. 编译时没有内置公钥的 Agent：拒绝升级"
install_agent $D/dist/op-agent-nokey
start_agent nokey
sleep 3
upgrade $V2
systemctl stop op-$RUN-nokey

echo "== 2. 装 $V1"
install_agent $D/dist/op-agent-$V1-amd64
start_agent agent
sleep 3
hellos master
nft list table inet $TABLE | grep -c redirect
echo "== 3. SHA-256 不对、签名不对、文件不存在：都拒绝，什么都不变"
upgrade $V2 '&bad=sha256'
upgrade $V2 '&bad=signature'
upgrade 9.9.9
versions
echo "== 4. 升级到 $V2"
upgrade $V2
sleep 8
grep -oE '"(upgradeResult|trafficReport)"' $D/master.log | tail -3 | paste -sd ' '
hellos master
versions

echo "== 5. 升级到 $V3（假主控拒绝它）：试运行 3 分钟没通过认证，换回 $V2"
upgrade $V3
sleep 8
hellos master
versions
echo "-- 等试运行超时（200 秒）"
sleep 200
hellos master
versions
grep -E '换回旧版本|升级标记' $D/agent.log | cut -c1-160

echo "== 6. 卸载"
curl -s -X POST -d '{"uninstall":{}}' "http://127.0.0.1:21000/ctl/request" | tr -d '\n '; echo
sleep 5
echo "服务：$(systemctl is-active op-$RUN-agent)"
files
nft list table inet $TABLE 2>&1 | head -1
grep -E '卸载|systemd' $D/agent.log | tail -4 | cut -c1-200

echo "== 7. 「服务器已删除」：重新装上，假主控回 SERVER_DELETED"
systemctl stop op-$RUN-master
start master2 $FM -listen 127.0.0.1:21000 -token $TOKEN -hello-status deleted
sleep 1
install_agent $D/dist/op-agent-$V2-amd64
start_agent agent2
sleep 5
echo "服务：$(systemctl is-active op-$RUN-agent2)"
files
grep -E '删除|卸载' $D/agent2.log | tail -3 | cut -c1-200

echo "== Agent 日志里的错误"
cat $D/nokey.log $D/agent.log $D/agent2.log | grep -E 'level=ERROR' | cut -c1-200
