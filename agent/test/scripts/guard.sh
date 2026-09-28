#!/bin/bash
# 技术债 agent-5 的运行验证：代理用户不能经节点访问节点服务器本机和内网（调试记录 R20260927-agent-08）。
# 在测试 VPS 上以 root 执行：
#   bash /opt/open-proxy/src/agent/agent/test/scripts/guard.sh <run-id> <开发电脑名>
# 前提：/opt/open-proxy/src/agent 已切到要测的提交。
# 端口只用 22000–22999（21000–21999、28080、28443 留给主控的测试）；回显服务监听 0.0.0.0:22997，
# 测试专用表 inet open_proxy_test 里的规则挡住从外网进来的包；所有进程在 open-proxy.slice 里；
# 结束时停掉本轮的进程、删表、删测试锁。测试锁被占用时退出码为 3，隔几分钟再跑。
set -u
RUN=${1:?用法: guard.sh <run-id> <开发电脑名>}
PC=${2:-unknown}
ROOT=/opt/open-proxy
D=$ROOT/runs/$RUN
LOCK=$ROOT/LOCK
TABLE=open_proxy_test
PUBIP=${PUBIP:?需要用环境变量 PUBIP 传入测试 VPS 的公网 IP}
ECHO=22997

if [ -e $LOCK ]; then
  echo "测试锁被占用：$(cat $LOCK)"
  exit 3
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

for p in cmd/op-agent test/fakemaster test/proxyclient; do
  run go build -p 6 -tags with_quic,with_utls -trimpath -o $ROOT/bin/$(basename $p)-agent-$H ./$p || exit 1
done
AGENT=$ROOT/bin/op-agent-agent-$H
FM=$ROOT/bin/fakemaster-agent-$H
PCLI=$ROOT/bin/proxyclient-agent-$H

rnd() { od -An -tx1 -N16 /dev/urandom | tr -d ' \n'; }
key16() { head -c16 /dev/urandom | base64; }
TOKEN=$(rnd)
PW=$(rnd)
LPW=$(rnd)
UUID=$(cat /proc/sys/kernel/random/uuid)
SSSERVER=$(key16)
SSUSER=$(key16)
SID=$(rnd | cut -c1-8)
eval "$(run $PCLI -reality-keygen)" # 设置 REALITY_PRIVATE、REALITY_PUBLIC
printf 'MASTER_URL=http://127.0.0.1:22000\nTOKEN=%s\n' $TOKEN > $D/op-agent.conf
chmod 600 $D/op-agent.conf

ss="\"shadowsocks2022\":{\"method\":\"2022-blake3-aes-128-gcm\",\"serverKey\":\"$SSSERVER\"}"
cat > $D/state.json <<EOF
{
  "users": [{"id": "1", "uuid": "$UUID", "password": "$PW", "ssKey": "$SSUSER"}],
  "nodes": [
    {"id": "1", "port": 22001, "userIds": ["1"], $ss},
    {"id": "2", "port": 22002, "userIds": ["1"], "vlessReality": {"privateKey": "$REALITY_PRIVATE", "shortIds": ["$SID"], "targetHost": "www.microsoft.com", "targetPort": 443}},
    {"id": "3", "port": 22003, "userIds": ["1"], "hysteria2": {}},
    {"id": "4", "port": 22004, "userIds": ["1"], "anytls": {}},
    {"id": "5", "port": 22005, "userIds": ["1"], "mieru": {}},
    {"id": "6", "port": 22006, "userIds": ["1"], "exitId": "1", $ss}
  ],
  "exits": [{"id": "1", "host": "127.0.0.1", "port": 22100, "username": "landing", "password": "$LPW"}],
  "landing": {"port": 22100, "username": "landing", "password": "$LPW", "allowedSourceIps": ["127.0.0.1"]}
}
EOF

# 回显服务监听 0.0.0.0，只挡从外网进来的：本机发往 $PUBIP 的包走 lo
nft add table inet $TABLE
nft add chain inet $TABLE input '{ type filter hook input priority 0; }'
nft add rule inet $TABLE input 'iifname != "lo"' tcp dport $ECHO drop
nft add rule inet $TABLE input 'iifname != "lo"' udp dport $ECHO drop

start echo $PCLI -echo-server 0.0.0.0:$ECHO
start master $FM -listen 127.0.0.1:22000 -token $TOKEN -state $D/state.json
sleep 1
start agent $AGENT -config $D/op-agent.conf -data-dir $D/data -nft-table $TABLE -log-level debug
sleep 5

echo "== 状态上报（应该没有失败项）"
grep stateReport $D/master.log | tail -1 | cut -c1-600
echo "== 对照：不经代理时回显服务在 127.0.0.1 和 $PUBIP 上都能用"
for ip in 127.0.0.1 $PUBIP; do
  if exec 3<>/dev/tcp/$ip/$ECHO && echo ping >&3 && read -t 3 line <&3 && [ "$line" = ping ]; then
    echo "TCP $ip:$ECHO 通"
  else
    echo "TCP $ip:$ECHO 不通"
  fi
  exec 3<&- 3>&-
done
before=$(grep -c '^收到' $D/echo.log)

# check <名称> <期望：成功|失败> <proxyclient 参数...>：每个检查一行，写进结果表
FAILS=0
ROWS=$D/rows.txt
: > $ROWS
check() {
  local name=$1 want=$2
  shift 2
  run $PCLI -timeout 3s "$@" > $D/last.txt 2>&1
  local got=0
  while read -r _ kind target result ms detail; do
    got=1
    local w=$want
    # 同一个 UDP 会话里：DNS 应该成功，发往回显服务的应该失败
    [ "$want" = 混合 ] && { [ "$kind" = dns ] && w=成功 || w=失败; }
    local mark=符合
    [ "$result" != "$w" ] && { mark=不符合; FAILS=$((FAILS + 1)); }
    echo "| $name | $kind | $target | $w | $result | $ms | $mark |" >> $ROWS
  done < <(grep '^结果' $D/last.txt)
  if [ $got = 0 ]; then
    FAILS=$((FAILS + 1))
    echo "| $name | ? | $* | $want | 没有结果 | | 不符合 |" >> $ROWS
    sed 's/^/    /' $D/last.txt | tail -3
  fi
}

declare -A ARGS
ARGS[ss2022]="-proto shadowsocks -server 127.0.0.1:22001 -password $SSSERVER:$SSUSER"
ARGS[vless-reality]="-proto vless -server 127.0.0.1:22002 -uuid $UUID -sni www.microsoft.com -reality-public-key $REALITY_PUBLIC -short-id $SID"
ARGS[hysteria2]="-proto hysteria2 -server 127.0.0.1:22003 -password $PW"
ARGS[anytls]="-proto anytls -server 127.0.0.1:22004 -password $PW"
ARGS[mieru]="-proto mieru -server 127.0.0.1:22005 -user 1 -password $PW"
ARGS[ss2022-exit]="-proto shadowsocks -server 127.0.0.1:22006 -password $SSSERVER:$SSUSER"

for p in ss2022 vless-reality hysteria2 anytls mieru; do
  a=${ARGS[$p]}
  echo "== $p"
  check $p 失败 $a -tcp-echo 127.0.0.1:$ECHO
  check $p 失败 $a -tcp-echo localhost:$ECHO
  check $p 失败 $a -tcp-echo $PUBIP:$ECHO
  check $p 失败 $a -tcp-echo 10.0.0.1:$ECHO
  check $p 成功 $a -http http://example.com/
  check $p 成功 $a -http http://1.1.1.1/
  check $p 成功 $a -dns 1.1.1.1:53
  check $p 失败 $a -udp-echo 127.0.0.1:$ECHO
  check $p 失败 $a -udp-echo localhost:$ECHO
  check $p 失败 $a -udp-echo $PUBIP:$ECHO
  check $p 混合 $a -dns 1.1.1.1:53 -udp-echo 127.0.0.1:$ECHO
done
echo "== ss2022 经落地出口（落地是本机的自建落地 127.0.0.1:22100）"
a=${ARGS[ss2022-exit]}
check ss2022-exit 失败 $a -tcp-echo 127.0.0.1:$ECHO
check ss2022-exit 失败 $a -tcp-echo localhost:$ECHO
check ss2022-exit 成功 $a -http http://example.com/
check ss2022-exit 成功 $a -dns 1.1.1.1:53
check ss2022-exit 失败 $a -udp-echo 127.0.0.1:$ECHO

after=$(grep -c '^收到' $D/echo.log)
echo "== 回显服务在默认模式下收到的经代理来的连接和包：$((after - before)) 个（应该是 0）"
grep '^收到' $D/echo.log | tail -n +$((before + 1)) | head -5

echo "== 一个会被 sing-box 记错误日志的连接（公网地址上没开的端口），看日志里有没有终端转义字符"
check ss2022 失败 ${ARGS[ss2022]} -tcp-echo example.com:81
sleep 8 # 等 sing-box 自己的拨号超时、打出错误日志
echo "Agent 日志里的 ESC 字符：$(grep -c $'\x1b' $D/agent.log) 行（应该是 0）"
grep -v 'level=' $D/agent.log | tail -3 | cut -c1-200
echo "== Agent 的拒绝日志"
echo "逐条记的：$(grep -c 'msg=拒绝连接' $D/agent.log) 条；限流后只记总数的：$(grep -c 没有逐条记 $D/agent.log) 次"
grep 'msg=拒绝连接' $D/agent.log | head -8 | cut -c1-260
echo "== Agent 日志（警告以上）"
grep -E 'level=(WARN|ERROR)' $D/agent.log | tail -10 | cut -c1-200

echo "== 带 -allow-private-targets 重启 Agent（只给测试用）：本机和内网的目标放行"
systemctl stop op-$RUN-agent
start agent2 $AGENT -config $D/op-agent.conf -data-dir $D/data -nft-table $TABLE -log-level debug -allow-private-targets
sleep 5
grep 'allow-private-targets' $D/agent2.log | cut -c1-200
for p in ss2022 vless-reality hysteria2 anytls mieru; do
  a=${ARGS[$p]}
  check "$p（放行）" 成功 $a -tcp-echo 127.0.0.1:$ECHO
  check "$p（放行）" 成功 $a -tcp-echo localhost:$ECHO
  check "$p（放行）" 成功 $a -tcp-echo $PUBIP:$ECHO
  check "$p（放行）" 成功 $a -udp-echo 127.0.0.1:$ECHO
done
echo "拒绝日志：$(grep -c 'msg=拒绝连接' $D/agent2.log) 条（应该是 0）"

echo "== 结果（期望和实际不一致的有 $FAILS 项）"
echo "| 节点 | 检查 | 目标 | 期望 | 实际 | 耗时 | |"
echo "| --- | --- | --- | --- | --- | --- | --- |"
cat $ROWS
