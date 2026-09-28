#!/usr/bin/env python3
"""主控 + Agent 的端到端测试：在一台机器上跑主控、Agent 和 sing-box 客户端，检查
期望状态下发、各协议连通、流量入账、停用和恢复、超额停用。

用法（在测试 VPS 上，整个脚本放进 open-proxy.slice 里跑）：
  python3 e2e_agent.py --master <op-master> --agent <op-agent> --singbox <sing-box> --workdir <空目录>

端口：主控 28080，客户端 SOCKS5 28101 起，本机下载服务 29999，节点在 21000–21999。
只用 Python 标准库；凭据直接从主控的 SQLite 读（订阅接口之前的阶段没有）。
"""

import argparse
import http.server
import json
import os
import sqlite3
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request

MASTER = "http://127.0.0.1:28080"
PAYLOAD_SIZE = 20 * 2**20
NL = chr(10)
COOKIE = {"value": ""}
PASSED = []
PROCS = []


def call(method, path, body=None, expect=200):
    data = json.dumps(body if body is not None else {}).encode() if method != "GET" else None
    req = urllib.request.Request(MASTER + path, data=data, method=method)
    if method != "GET":
        req.add_header("Content-Type", "application/json")
    if COOKIE["value"]:
        req.add_header("Cookie", COOKIE["value"])
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            status, headers, raw = resp.status, resp.headers, resp.read()
    except urllib.error.HTTPError as err:
        status, headers, raw = err.code, err.headers, err.read()
    for value in headers.get_all("Set-Cookie") or []:
        pair = value.split(";", 1)[0]
        if pair.startswith("op_session="):
            COOKIE["value"] = pair
    payload = json.loads(raw) if raw else None
    if status != expect:
        raise AssertionError(f"{method} {path}：期望 {expect}，实际 {status}，返回 {payload}")
    return payload


def check(name, cond, detail=""):
    if not cond:
        raise AssertionError(f"{name} 不成立 {detail}")
    PASSED.append(name)
    print(f"  ✓ {name}", flush=True)


def wait_until(what, fn, timeout=30, interval=1):
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        last = fn()
        if last:
            return last
        time.sleep(interval)
    raise AssertionError(f"等待「{what}」超时，最后一次结果：{last}")


def start(name, args, workdir):
    log = open(os.path.join(workdir, f"{name}.log"), "wb")
    proc = subprocess.Popen(args, stdout=log, stderr=subprocess.STDOUT)
    PROCS.append(proc)
    return proc


def serve_payload():
    """本机下载服务：GET /payload 返回 20 MiB。"""
    body = os.urandom(PAYLOAD_SIZE)

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 29999), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()


def fetch_via(socks_port, timeout=30):
    """经过本机的 SOCKS5 下载 payload，返回下载的字节数；失败返回 0。"""
    result = subprocess.run(
        ["curl", "-s", "-o", "/dev/null", "-w", "%{size_download}", "--max-time", str(timeout),
         "--socks5-hostname", f"127.0.0.1:{socks_port}", "http://127.0.0.1:29999/payload"],
        capture_output=True, text=True)
    try:
        return int(result.stdout.strip() or 0)
    except ValueError:
        return 0


def server_status(sid):
    return call("GET", f"/api/servers/{sid}")


def wait_applied(sid, before, what):
    """等期望状态升过版本并被 Agent 应用。"""
    return wait_until(what, lambda: (lambda s: s if s["state_version"] > before
                                     and s["applied_version"] == s["state_version"] else None)(server_status(sid)),
                      timeout=20)


def synced(sid):
    s = server_status(sid)
    return s if s["online"] and s["state_version"] and s["applied_version"] == s["state_version"] else None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--master", required=True)
    ap.add_argument("--agent", required=True)
    ap.add_argument("--singbox", required=True)
    ap.add_argument("--mihomo", help="mihomo 客户端；给了就用订阅里的 Mihomo 配置逐个节点测连通")
    ap.add_argument("--workdir", required=True)
    args = ap.parse_args()
    work = os.path.abspath(args.workdir)
    os.makedirs(work, exist_ok=True)

    print("== 启动主控", flush=True)
    start("master", [args.master, "--data-dir", f"{work}/master-data", "--no-https",
                     "--http-listen", "127.0.0.1:28080", "--public-url", MASTER], work)
    wait_until("主控启动", lambda: _alive(), timeout=15)
    call("POST", "/api/setup", {"username": "admin", "password": "e2e-pass-123"})

    server = call("POST", "/api/servers", {"name": "本机", "address": "127.0.0.1",
                                           "port_range_start": 21000, "port_range_end": 21999})
    token = server["install_command"].split()[-1]
    sid = server["server"]["id"]
    nodes = {
        "ss": call("POST", "/api/nodes", {"server_id": sid, "name": "SS", "protocol": "shadowsocks2022"}),
        "vless": call("POST", "/api/nodes", {"server_id": sid, "name": "VLESS", "protocol": "vless_reality",
                                             "reality_target": "www.microsoft.com"}),
        "hy2": call("POST", "/api/nodes", {"server_id": sid, "name": "Hy2", "protocol": "hysteria2", "obfs": True}),
        "anytls": call("POST", "/api/nodes", {"server_id": sid, "name": "AnyTLS", "protocol": "anytls"}),
        "mieru": call("POST", "/api/nodes", {"server_id": sid, "name": "Mieru", "protocol": "mieru"}),
    }
    plan = call("POST", "/api/plans", {"name": "测试", "traffic_quota_bytes": None,
                                       "node_ids": [n["id"] for n in nodes.values()]})
    alice = call("POST", "/api/users", {"name": "alice", "plan_id": plan["id"]})
    bob = call("POST", "/api/users", {"name": "bob", "plan_id": plan["id"]})

    print("== 启动 Agent", flush=True)
    with open(f"{work}/op-agent.conf", "w") as f:
        f.write(f"MASTER_URL=http://127.0.0.1:28080\nTOKEN={token}\n")
    start("agent", [args.agent, "-config", f"{work}/op-agent.conf", "-data-dir", f"{work}/agent-data"], work)
    s = wait_until("Agent 上线并应用期望状态", lambda: synced(sid), timeout=40)
    check("Agent 上线、版本一致、已应用最新状态", s["online"] and not s["version_mismatch"])
    failures = s["apply_failures"]
    print(f"    应用失败项：{failures}", flush=True)
    check("除 Mieru 以外没有失败项（Mieru 在 M5 实现前会失败）",
          all(f["item"] == "node" and f["id"] == nodes["mieru"]["id"] for f in failures))

    # 凭据和节点参数直接读主控的数据库
    db = sqlite3.connect(f"{work}/master-data/op-master.db")
    cred = dict((r[0], r[1:]) for r in db.execute("SELECT id, uuid, password, ss_key FROM users"))
    params = dict((r[0], json.loads(r[1])) for r in db.execute("SELECT id, params FROM nodes"))
    db.close()

    def client_config(user_id):
        uuid, password, ss_key = cred[user_id]
        p = {k: params[n["id"]] for k, n in nodes.items()}
        outbounds = [
            {"type": "shadowsocks", "tag": "ss", "server": "127.0.0.1", "server_port": nodes["ss"]["port"],
             "method": "2022-blake3-aes-128-gcm", "password": f"{p['ss']['server_key']}:{ss_key}"},
            {"type": "vless", "tag": "vless", "server": "127.0.0.1", "server_port": nodes["vless"]["port"],
             "uuid": uuid, "flow": "xtls-rprx-vision",
             "tls": {"enabled": True, "server_name": "www.microsoft.com",
                     "utls": {"enabled": True, "fingerprint": "chrome"},
                     "reality": {"enabled": True, "public_key": p["vless"]["public_key"],
                                 "short_id": p["vless"]["short_ids"][0]}}},
            {"type": "hysteria2", "tag": "hy2", "server": "127.0.0.1", "server_port": nodes["hy2"]["port"],
             "password": password, "obfs": {"type": "salamander", "password": p["hy2"]["obfs_password"]},
             "tls": {"enabled": True, "server_name": "open-proxy", "insecure": True}},
            {"type": "anytls", "tag": "anytls", "server": "127.0.0.1", "server_port": nodes["anytls"]["port"],
             "password": password, "tls": {"enabled": True, "server_name": "open-proxy", "insecure": True}},
        ]
        base = 28101 if user_id == alice["id"] else 28111
        inbounds = [{"type": "socks", "tag": f"in-{o['tag']}", "listen": "127.0.0.1", "listen_port": base + i}
                    for i, o in enumerate(outbounds)]
        rules = [{"inbound": [f"in-{o['tag']}"], "outbound": o["tag"]} for o in outbounds]
        return {"log": {"level": "warn"}, "inbounds": inbounds, "outbounds": outbounds,
                "route": {"rules": rules}}, {o["tag"]: base + i for i, o in enumerate(outbounds)}

    serve_payload()
    ports = {}
    for name, user in (("alice", alice), ("bob", bob)):
        config, ports[name] = client_config(user["id"])
        path = f"{work}/client-{name}.json"
        with open(path, "w") as f:
            json.dump(config, f)
        start(f"client-{name}", [args.singbox, "run", "-c", path], work)
    time.sleep(2)

    print("== 各协议连通", flush=True)
    for proto, port in ports["alice"].items():
        got = fetch_via(port)
        check(f"alice 经 {proto} 下载 20 MiB", got == PAYLOAD_SIZE, f"实际 {got} 字节")
    check("bob 经 SS 下载", fetch_via(ports["bob"]["ss"]) == PAYLOAD_SIZE)

    print("== 流量入账", flush=True)
    def alice_counted():
        u = call("GET", f"/api/users/{alice['id']}")
        return u if u["down_total"] >= 4 * PAYLOAD_SIZE else None
    u = wait_until("alice 的流量入账", alice_counted, timeout=30)
    check("alice 的下行在 4×20 MiB 左右", 4 * PAYLOAD_SIZE <= u["down_total"] < 4 * PAYLOAD_SIZE * 1.05,
          f"实际 {u['down_total']}")
    rows = call("GET", f"/api/users/{alice['id']}/traffic")["rows"]
    check("alice 的日账本按节点分开", {r["node_id"] for r in rows} >=
          {nodes[k]["id"] for k in ("ss", "vless", "hy2", "anytls")})
    ov = call("GET", "/api/overview")
    check("概览：今天的流量和在线服务器", ov["today"]["down"] >= 5 * PAYLOAD_SIZE and ov["servers"]["online"] == 1)
    s = server_status(sid)
    print(f"    网速 rx={s['rx_speed']} tx={s['tx_speed']}，本月网卡 rx={s['month_rx']} tx={s['month_tx']}", flush=True)

    print("== 订阅", flush=True)
    test_subscription(args, work, alice, bob, nodes, failures)

    print("== 停用和恢复", flush=True)
    before = server_status(sid)["state_version"]
    call("PATCH", f"/api/users/{alice['id']}", {"enabled": False})
    wait_until("停用后期望状态升版本并应用",
               lambda: (lambda s: s if s["state_version"] > before and s["applied_version"] == s["state_version"] else None)(server_status(sid)),
               timeout=20)
    check("alice 手动停用", call("GET", f"/api/users/{alice['id']}")["blocked_reason"] == "manual")
    check("停用后 alice 经 SS 连不上", fetch_via(ports["alice"]["ss"], timeout=8) == 0)
    check("停用后 alice 经 VLESS 连不上", fetch_via(ports["alice"]["vless"], timeout=8) == 0)
    check("bob 不受影响", fetch_via(ports["bob"]["ss"]) == PAYLOAD_SIZE)
    before = server_status(sid)["state_version"]
    call("PATCH", f"/api/users/{alice['id']}", {"enabled": True})
    wait_until("恢复后应用", lambda: (lambda s: s if s["state_version"] > before and s["applied_version"] == s["state_version"] else None)(server_status(sid)), timeout=20)
    check("恢复后 alice 能用", fetch_via(ports["alice"]["ss"]) == PAYLOAD_SIZE)

    print("== 超额停用", flush=True)
    call("PATCH", f"/api/plans/{plan['id']}", {"traffic_quota_bytes": 50 * 2**20})
    wait_until("alice 超额停用", lambda: call("GET", f"/api/users/{alice['id']}")["blocked_reason"] == "over_quota", timeout=20)
    check("alice 因超额停用", True)
    before = server_status(sid)["state_version"]
    call("POST", f"/api/users/{alice['id']}/reset-period")
    wait_until("清零后恢复", lambda: call("GET", f"/api/users/{alice['id']}")["blocked_reason"] == "", timeout=20)
    wait_applied(sid, before, "清零后的期望状态被应用")
    check("清零本周期用量后恢复，Agent 已应用", fetch_via(ports["alice"]["ss"]) == PAYLOAD_SIZE)
    before = server_status(sid)["state_version"]
    fetch_via(ports["alice"]["ss"])
    fetch_via(ports["alice"]["ss"])
    wait_until("再次超额", lambda: call("GET", f"/api/users/{alice['id']}")["blocked_reason"] == "over_quota", timeout=30)
    wait_applied(sid, before, "超额后的期望状态被应用")
    check("超额后连不上", fetch_via(ports["alice"]["ss"], timeout=8) == 0)

    s = server_status(sid)
    print(f"    网卡：网速 rx={s['rx_speed']} tx={s['tx_speed']} 字节/秒，本月 rx={s['month_rx']} tx={s['month_tx']} 字节"
          "（测试流量走本机回环，不经过网卡）", flush=True)

    print(f"== 全部通过（{len(PASSED)} 项）", flush=True)


def fetch_sub(url, ua=None, fmt=None):
    """取订阅，返回 (状态码, 响应头, 正文)。"""
    if fmt:
        url += ("&" if "?" in url else "?") + f"format={fmt}"
    req = urllib.request.Request(url)
    if ua:
        req.add_header("User-Agent", ua)
    with urllib.request.urlopen(req, timeout=30) as resp:
        return resp.status, resp.headers, resp.read().decode()


def test_subscription(args, work, alice, bob, nodes, failures):
    import base64
    import urllib.parse
    sub = alice["sub_url"]
    status, headers, body = fetch_sub(sub, ua="clash-verge/v2.0 mihomo/1.19")
    check("Mihomo：YAML，五个节点加两个提示节点",
          "text/yaml" in headers["Content-Type"] and body.count(NL + "  - name: ") >= 7
          and "剩余流量 不限" in body and "到期 永久" in body and "type: mieru" in body)
    check("额度不限时不发 subscription-userinfo", headers.get("subscription-userinfo") is None)
    check("profile-title、更新间隔", headers["profile-title"].startswith("base64:")
          and headers["profile-update-interval"] == "24")
    with open(f"{work}/sub-mihomo.yaml", "w") as f:
        f.write(body)

    expect = {
        "Stash/2.4": ("stash", "type: vless", "type: mieru", "auth:"),
        "Shadowrocket/2.2": ("shadowrocket", "type: anytls", "type: mieru", None),
        "Surge iOS/3000": ("surge", "= anytls,", "= vless", None),
        "Loon/3.2": ("loon", "= vless,", "mieru", None),
        "Quantumult%20X/1.5": ("quantumultx", "shadowsocks=", "hysteria2", None),
    }
    for ua, (name, must, must_not, extra) in expect.items():
        _, headers, body = fetch_sub(sub, ua=ua)
        with open(f"{work}/sub-{name}.txt", "w") as f:
            f.write(body)
        ok = must in body and must_not not in body and (extra is None or extra in body)
        check(f"{name}：按 UA 识别，节点按能力表过滤", ok, body[:300])
    _, _, body = fetch_sub(sub, fmt="v2ray")
    links = base64.b64decode(body).decode().splitlines()
    kinds = sorted({l.split("://")[0] for l in links})
    check("分享链接：base64，五种协议加提示节点", len(links) == 7 and kinds ==
          ["anytls", "hysteria2", "mieru", "ss", "vless"], links)
    _, _, body = fetch_sub(BASE_SUB_INVALID)
    check("无效链接回提示节点", "订阅链接无效" in body)

    if not args.mihomo:
        return
    # 用 mihomo 跑订阅里的 Mihomo 配置：本机下载服务的请求加一条规则走「手动选择」，逐个切换节点
    text = open(f"{work}/sub-mihomo.yaml").read()
    text = text.replace("mixed-port: 7890", "mixed-port: 28131")
    text = text.replace(NL + "rules:" + NL, NL + "rules:" + NL + "  - DST-PORT,29999,手动选择" + NL, 1)
    text += "external-controller: 127.0.0.1:28132" + NL
    os.makedirs(f"{work}/mihomo", exist_ok=True)
    with open(f"{work}/mihomo/config.yaml", "w") as f:
        f.write(text)
    t = subprocess.run([args.mihomo, "-t", "-d", f"{work}/mihomo"], capture_output=True, text=True)
    check("mihomo 校验订阅配置", t.returncode == 0, t.stdout[-500:] + t.stderr[-500:])
    start("mihomo", [args.mihomo, "-d", f"{work}/mihomo"], work)
    time.sleep(3)
    failed = {f["id"] for f in failures}
    group = urllib.parse.quote("手动选择")
    for key, node in nodes.items():
        if node["id"] in failed:
            continue
        req = urllib.request.Request(f"http://127.0.0.1:28132/proxies/{group}", method="PUT",
                                     data=json.dumps({"name": node["name"]}).encode())
        req.add_header("Content-Type", "application/json")
        urllib.request.urlopen(req, timeout=10).read()
        got = fetch_via(28131)
        check(f"mihomo 经订阅里的 {key} 节点下载", got == PAYLOAD_SIZE, f"实际 {got}")


BASE_SUB_INVALID = MASTER + "/s/not-a-valid-token"


def _alive():
    try:
        return call("GET", "/api/health")
    except Exception:
        return None


if __name__ == "__main__":
    code = 0
    try:
        main()
    except AssertionError as err:
        print(f"✗ {err}", flush=True)
        code = 1
    finally:
        for proc in PROCS:
            proc.terminate()
        for proc in PROCS:
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
    sys.exit(code)
