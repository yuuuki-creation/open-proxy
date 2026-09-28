#!/bin/bash
# M5 Mieru 节点的运行验证（调试记录 R20260927-agent-01）。在测试 VPS 上以 root 执行：
#   bash /opt/open-proxy/src/agent/agent/test/scripts/m5-mieru.sh <run-id> <开发电脑名>
# 前提：/opt/open-proxy/src/agent 已切到要测的提交。
# 只用本机回环地址和 21000–21999 端口；所有进程在 open-proxy.slice 里；结束时停掉本轮的进程、删测试锁。
set -u
RUN=${1:?用法: m5-mieru.sh <run-id> <开发电脑名>}
PC=${2:-unknown}
ROOT=/opt/open-proxy
D=$ROOT/runs/$RUN
LOCK=$ROOT/LOCK

if [ -e $LOCK ]; then
  echo "测试锁被占用：$(cat $LOCK)"
  exit 1
fi
echo "$RUN agent $PC $(date -Is)" > $LOCK
cleanup() {
  systemctl stop "op-$RUN-*" 2>/dev/null
  systemctl reset-failed "op-$RUN-*" 2>/dev/null
  rm -f $LOCK
}
trap cleanup EXIT

mkdir -p $D/data
. $ROOT/env.sh
cd $ROOT/src/agent/agent || exit 1
H=$(git rev-parse --short HEAD </dev/null)
echo "提交 $H，结果目录 $D"

# 前台命令也要在 slice 里跑
run() { systemd-run --quiet --scope --slice=open-proxy.slice -- "$@" </dev/null; }
# 后台进程：单元名 op-<run-id>-<名称>，输出写到结果目录
start() {
  local name=$1
  shift
  systemd-run --quiet --unit=op-$RUN-$name --slice=open-proxy.slice \
    -p StandardOutput=file:$D/$name.log -p StandardError=file:$D/$name.log "$@"
}

for p in cmd/op-agent test/fakemaster test/mieruclient; do
  run go build -p 6 -tags with_quic,with_utls -trimpath -o $ROOT/bin/$(basename $p)-agent-$H ./$p || exit 1
done
AGENT=$ROOT/bin/op-agent-agent-$H
FM=$ROOT/bin/fakemaster-agent-$H
MC=$ROOT/bin/mieruclient-agent-$H

rnd() { od -An -tx1 -N16 /dev/urandom | tr -d ' \n'; }
TOKEN=$(rnd); P1=$(rnd); P2=$(rnd); P3=$(rnd); P1B=$(rnd)
printf 'MASTER_URL=http://127.0.0.1:21000\nTOKEN=%s\n' $TOKEN > $D/op-agent.conf
chmod 600 $D/op-agent.conf

node='"id":"1","port":21001,"mieru":{}'
echo "{\"users\":[{\"id\":\"1\",\"password\":\"$P1\"},{\"id\":\"2\",\"password\":\"$P2\"}],\"nodes\":[{$node,\"userIds\":[\"1\",\"2\"]}]}" > $D/state1.json
echo "{\"users\":[{\"id\":\"1\",\"password\":\"$P1\"},{\"id\":\"3\",\"password\":\"$P3\"}],\"nodes\":[{$node,\"userIds\":[\"1\",\"3\"]}]}" > $D/state2.json
echo "{\"users\":[{\"id\":\"1\",\"password\":\"$P1B\"},{\"id\":\"3\",\"password\":\"$P3\"}],\"nodes\":[{$node,\"userIds\":[\"1\",\"3\"]}]}" > $D/state3.json
echo '{}' > $D/state4.json

URL=http://www.gstatic.com/generate_204
mc() { run $MC -server 127.0.0.1:21001 "$@"; }
push() { curl -s --data-binary @$D/$1 http://127.0.0.1:21000/ctl/state; }
last_report() { grep stateReport $D/master.log | tail -1; }

start master $FM -listen 127.0.0.1:21000 -token $TOKEN -state $D/state1.json
start echo $MC -echo-server 127.0.0.1:21999
sleep 1
start agent $AGENT -config $D/op-agent.conf -data-dir $D/data
sleep 3

echo "== 1. 监听（应该只有 IPv4 的 0.0.0.0:21001）"
ss -ltnp | grep ':21001 ' || echo "没有监听 21001"
echo "== 2. 另一个设了 SO_REUSEPORT 的套接字能不能绑同一个端口（应该失败）"
run python3 -c '
import socket
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1)
try:
    s.bind(("0.0.0.0", 21001))
    print("绑定成功：Agent 的监听设了 SO_REUSEPORT")
except OSError as e:
    print("绑定失败：", e)
'
echo "== 3. 状态上报"
last_report
echo "== 4. 用户 1 访问 HTTP（CONNECT）"
mc -user 1 -password $P1 -http $URL -count 2
echo "== 5. 用户 1 查 DNS（UDP ASSOCIATE）"
mc -user 1 -password $P1 -dns 1.1.1.1:53 -name example.com -count 2
echo "== 6. 用户 1 用错的密码"
mc -user 1 -password wrong -http $URL
echo "== 7. 流量上报（等 11 秒）"
sleep 11
grep trafficReport $D/master.log | tail -1

echo "== 8. 只增删用户（删 2 加 3）：热更新，用户 1 的连接不断，用户 2 的连接被断开"
start echo1 $MC -server 127.0.0.1:21001 -user 1 -password $P1 -echo 127.0.0.1:21999 -count 12
start echo2 $MC -server 127.0.0.1:21001 -user 2 -password $P2 -echo 127.0.0.1:21999 -count 12
sleep 4
push state2.json
sleep 10
echo "-- 用户 1 的回显"
cat $D/echo1.log
echo "-- 用户 2 的回显"
cat $D/echo2.log
last_report
echo "-- 用户 3 访问 HTTP（应该成功）"
mc -user 3 -password $P3 -http $URL
echo "-- 用户 2 访问 HTTP（应该失败）"
mc -user 2 -password $P2 -http $URL

echo "== 9. 用户 1 改密码：重建，旧密码建立的连接断开"
start echo1b $MC -server 127.0.0.1:21001 -user 1 -password $P1 -echo 127.0.0.1:21999 -count 10
sleep 3
push state3.json
sleep 9
cat $D/echo1b.log
last_report
echo "-- 新密码（应该成功）"
mc -user 1 -password $P1B -http $URL
echo "-- 旧密码（应该失败）"
mc -user 1 -password $P1 -http $URL

echo "== 10. 删掉节点：端口释放"
push state4.json
sleep 2
ss -ltnp | grep ':21001 ' || echo "已不再监听 21001"
last_report

echo "== 每次应用用了多久"
grep '期望状态已应用' $D/agent.log | grep -o 'version=[0-9]* failures=[0-9]* elapsed=[^ ]*'
echo "== Agent 日志（警告以上，含 sing-box 自己的日志）"
grep -vE 'level=(INFO|DEBUG)' $D/agent.log | tail -20
