#!/usr/bin/env python3
"""主控运维功能的端到端测试：安装脚本、二进制下载的认证、用安装命令装 Agent（systemd 服务）、
升级（拒绝、连不上主控时回滚、成功）、REALITY 检测和扫描、备份和恢复、删除服务器时 Agent 自己卸载。

用法（在测试 VPS 上用 root 跑，整个脚本放进 open-proxy.slice）：
  python3 e2e_ops.py --master <op-master> --agent-dir <目录> --old-agent <文件> --workdir <空目录>

- 主控用 OP_VERSION=<新版本> 编译
- --agent-dir：和主控同版本的 Agent（op-agent-linux-amd64）和签名（.sig），编译时注入测试公钥
- --old-agent：旧版本的 Agent，同样注入测试公钥。先装它，再升级到新版本

会在系统里装 op-agent 服务（systemd 单元带 Slice=open-proxy.slice，端口跳跃用测试专用的 nftables 表），
结束时如果还在就清理掉。REALITY 扫描只扫本机回环网段。回滚要等 Agent 的试运行超时（3 分钟）。
"""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request

MASTER = "http://127.0.0.1:28080"
NFT_TABLE = "open_proxy_test"
COOKIE = {"value": ""}
PASSED = []
PROCS = []


def call(method, path, body=None, expect=200, raw=False, headers=None, data=None):
    if data is None and method != "GET":
        data = json.dumps(body if body is not None else {}).encode()
    req = urllib.request.Request(MASTER + path, data=data, method=method)
    if method != "GET" and not (headers and "Content-Type" in headers):
        req.add_header("Content-Type", "application/json")
    for k, v in (headers or {}).items():
        req.add_header(k, v)
    if COOKIE["value"]:
        req.add_header("Cookie", COOKIE["value"])
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            status, hdrs, content = resp.status, resp.headers, resp.read()
    except urllib.error.HTTPError as err:
        status, hdrs, content = err.code, err.headers, err.read()
    for value in hdrs.get_all("Set-Cookie") or []:
        pair = value.split(";", 1)[0]
        if pair.startswith("op_session="):
            COOKIE["value"] = pair
    if status != expect:
        raise AssertionError(f"{method} {path}：期望 {expect}，实际 {status}，返回 {content[:300]!r}")
    if raw:
        return content
    return json.loads(content) if content else None


def check(name, cond, detail=""):
    if not cond:
        raise AssertionError(f"{name} 不成立 {detail}")
    PASSED.append(name)
    print(f"  ✓ {name}", flush=True)


def wait_until(what, fn, timeout=30, interval=1):
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        try:
            last = fn()
        except Exception as err:  # noqa: BLE001 主控重启中
            last = err
        if last and not isinstance(last, Exception):
            return last
        time.sleep(interval)
    raise AssertionError(f"等待「{what}」超时，最后一次结果：{last}")


def start_master(args, work, agent_dir):
    log = open(os.path.join(work, "master.log"), "ab")
    proc = subprocess.Popen([args.master, "--data-dir", f"{work}/master-data", "--no-https",
                             "--http-listen", "127.0.0.1:28080", "--public-url", MASTER,
                             "--agent-dir", agent_dir], stdout=log, stderr=subprocess.STDOUT)
    PROCS.append(proc)
    wait_until("主控启动", lambda: call("GET", "/api/health"), timeout=15)
    return proc


def stop_master(proc):
    proc.terminate()
    proc.wait(timeout=10)
    PROCS.remove(proc)


def restart_master(args, work, proc, agent_dir):
    stop_master(proc)
    proc = start_master(args, work, agent_dir)
    call("POST", "/api/auth/login", {"username": "admin", "password": "ops-pass-123"})
    return proc


def master_log(work):
    with open(os.path.join(work, "master.log"), "rb") as f:
        return f.read().decode(errors="replace")


def server_view(sid):
    return call("GET", f"/api/servers/{sid}")


def wait_server(sid, what, cond, timeout=60):
    return wait_until(what, lambda: (lambda s: s if cond(s) else None)(server_view(sid)), timeout=timeout)


def sha256_file(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def installed_version():
    r = subprocess.run(["/usr/local/bin/op-agent", "-version"], capture_output=True, text=True)
    return r.stdout.strip()


def agent_dir_of(work, name, binary, signature):
    """准备一个 --agent-dir：二进制和签名（signature 为 None 时不放签名）。"""
    d = os.path.join(work, name)
    os.makedirs(d, exist_ok=True)
    shutil.copyfile(binary, os.path.join(d, "op-agent-linux-amd64"))
    if signature is not None:
        with open(os.path.join(d, "op-agent-linux-amd64.sig"), "wb") as f:
            f.write(signature)
    return d


def cleanup_agent():
    subprocess.run(["systemctl", "disable", "--now", "op-agent"], capture_output=True)
    for path in ["/etc/systemd/system/op-agent.service", "/usr/local/bin/op-agent",
                 "/usr/local/bin/op-agent.bak", "/etc/op-agent/op-agent.conf"]:
        if os.path.exists(path):
            os.remove(path)
    subprocess.run(["rm", "-rf", "/var/lib/op-agent", "/etc/op-agent"])
    subprocess.run(["systemctl", "daemon-reload"])
    subprocess.run(["nft", "delete", "table", "inet", NFT_TABLE], capture_output=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--master", required=True)
    ap.add_argument("--agent-dir", required=True)
    ap.add_argument("--old-agent", required=True)
    ap.add_argument("--workdir", required=True)
    args = ap.parse_args()
    work = os.path.abspath(args.workdir)
    os.makedirs(work, exist_ok=True)
    new_binary = os.path.join(args.agent_dir, "op-agent-linux-amd64")
    new_version = subprocess.run([new_binary, "-version"], capture_output=True, text=True).stdout.strip()
    old_version = subprocess.run([args.old_agent, "-version"], capture_output=True, text=True).stdout.strip()
    # 主控只托管自己版本的 Agent。先让它托管旧版本的二进制（没有签名），模拟「装好之后主控升级了」
    old_dir = agent_dir_of(work, "agent-old", args.old_agent, None)
    # 签名是随机数据：Agent 应该拒绝
    bad_dir = agent_dir_of(work, "agent-bad-sig", new_binary, os.urandom(64))

    print(f"== 启动主控（旧 Agent {old_version}，新 Agent {new_version}）", flush=True)
    master = start_master(args, work, old_dir)
    version = call("GET", "/api/health")["version"]
    check("主控和新 Agent 同版本", version == new_version and old_version != new_version, (version, old_version))
    call("POST", "/api/setup", {"username": "admin", "password": "ops-pass-123"})
    server = call("POST", "/api/servers", {"name": "本机", "address": "127.0.0.1",
                                           "port_range_start": 21000, "port_range_end": 21999})
    sid = server["server"]["id"]
    token = server["install_command"].split()[-1]

    print("== 安装脚本和二进制下载", flush=True)
    script = call("GET", "/api/agent/install.sh", raw=True).decode()
    check("安装脚本填好了版本和 SHA-256", f'VERSION="{version}"' in script
          and f'SHA256_AMD64="{sha256_file(args.old_agent)}"' in script and "__" not in script)
    call("GET", f"/api/agent/binary/{version}/amd64", expect=401, raw=True)
    call("GET", f"/api/agent/binary/{version}/amd64", expect=401, raw=True,
         headers={"Authorization": "Bearer wrong-token"})
    call("GET", "/api/agent/binary/0.0.1/amd64", expect=404, raw=True,
         headers={"Authorization": f"Bearer {token}"})
    got = call("GET", f"/api/agent/binary/{version}/amd64", raw=True, headers={"Authorization": f"Bearer {token}"})
    check("带 Token 下载到的二进制和原文件一致", hashlib.sha256(got).hexdigest() == sha256_file(args.old_agent))

    print("== 用安装命令装 Agent", flush=True)
    cleanup_agent()
    env = f"OP_AGENT_SLICE=open-proxy.slice OP_AGENT_ARGS='-nft-table {NFT_TABLE}'"
    cmd = server["install_command"].replace("| bash -s --", f"| {env} bash -s --")
    result = subprocess.run(["bash", "-c", cmd], capture_output=True, text=True)
    print("    " + (result.stdout + result.stderr).strip().replace("\n", "\n    "), flush=True)
    check("安装命令执行成功", result.returncode == 0)
    unit = open("/etc/systemd/system/op-agent.service").read()
    check("systemd 服务在 open-proxy.slice 里", "Slice=open-proxy.slice" in unit)
    check("启动参数写进了服务", f"ExecStart=/usr/local/bin/op-agent -nft-table {NFT_TABLE}\n" in unit)
    check("配置文件权限 0600", oct(os.stat("/etc/op-agent/op-agent.conf").st_mode & 0o777) == "0o600")
    s = wait_server(sid, "Agent 上线", lambda s: s["online"])
    check("装好的 Agent 连上了主控", s["agent_arch"] == "amd64" and s["agent_version"] == old_version, s)
    check("版本不一致时暂停同步", s["version_mismatch"] is True)

    print("== 升级：拒绝的情况", flush=True)
    r = call("POST", f"/api/servers/{sid}/upgrade", expect=409)
    check("Agent 没有签名时主控不发升级指令", r["code"] == "unsigned", r)
    master = restart_master(args, work, master, bad_dir)
    wait_server(sid, "Agent 重新连上", lambda s: s["online"])
    call("POST", f"/api/servers/{sid}/upgrade")
    s = wait_server(sid, "升级失败的原因", lambda s: s["upgrade_error"])
    print(f"    Agent 的回复：{s['upgrade_error']}", flush=True)
    check("签名不对时 Agent 拒绝升级", s["online"] and s["agent_version"] == old_version)
    check("拒绝升级后二进制没变", sha256_file("/usr/local/bin/op-agent") == sha256_file(args.old_agent))

    print("== 升级后连不上主控：自动回滚（要等 3 分钟）", flush=True)
    master = restart_master(args, work, master, args.agent_dir)
    wait_server(sid, "Agent 重新连上", lambda s: s["online"])
    log_start = len(master_log(work))
    call("POST", f"/api/servers/{sid}/upgrade")
    wait_until("Agent 接受升级", lambda: "Agent 已接受升级" in master_log(work)[log_start:], timeout=60, interval=0.2)
    # 旧版本回复后退出，systemd 5 秒后拉起新版本；赶在新版本连上之前停掉主控
    stop_master(master)
    check("Agent 换上了新版本", wait_until("新版本启动", lambda: installed_version() == new_version, timeout=30))
    started = time.time()
    wait_until("回滚到旧版本", lambda: installed_version() == old_version, timeout=330, interval=5)
    print(f"    新版本启动后约 {time.time() - started:.0f} 秒换回旧版本", flush=True)
    check("连不上主控时换回旧版本", sha256_file("/usr/local/bin/op-agent") == sha256_file(args.old_agent))
    master = start_master(args, work, args.agent_dir)
    call("POST", "/api/auth/login", {"username": "admin", "password": "ops-pass-123"})
    s = wait_server(sid, "旧版本连上", lambda s: s["online"], timeout=120)
    check("主控看到上次升级回滚了", s["rolled_back_from"] == new_version and s["agent_version"] == old_version, s)

    print("== 升级成功", flush=True)
    r = call("POST", "/api/servers/upgrade-all")
    check("全部升级发出一条指令", r["started"] == 1, r)
    s = wait_server(sid, "新版本连上", lambda s: s["online"] and s["agent_version"] == new_version, timeout=90)
    check("升级后版本一致、恢复同步", s["version_mismatch"] is False and s["upgrade_error"] is None)
    check("升级成功后清掉回滚记录", s["rolled_back_from"] == "", s)
    check("升级后的二进制和主控托管的一致", sha256_file("/usr/local/bin/op-agent") == sha256_file(new_binary))
    check("保留了升级前的备份", sha256_file("/usr/local/bin/op-agent.bak") == sha256_file(args.old_agent))
    r = call("POST", "/api/servers/upgrade-all")
    check("版本一致时全部升级不发指令", r["started"] == 0, r)
    call("POST", "/api/nodes", {"server_id": sid, "name": "升级后", "protocol": "shadowsocks2022"})
    wait_server(sid, "Agent 应用新配置", lambda s: s["synced"] and s["applied_version"] > 0)
    check("升级后配置同步正常", True)

    print("== REALITY 检测", flush=True)
    r = call("POST", f"/api/servers/{sid}/reality/check",
             {"targets": ["www.microsoft.com", "www.apple.com:443", "no-such-host.invalid"]})
    for c in r["results"]:
        print(f"    {c['target']}: tls13={c['tls13']} h2={c['h2']} 证书有效={c['certificate_valid']} "
              f"延迟={c['latency_ms']}ms {c['error']}", flush=True)
    good = [c for c in r["results"][:2] if c["tls13"] and c["certificate_valid"] and not c["error"]]
    check("公网目标检测出 TLS 1.3 和有效证书", len(good) >= 1)
    check("不存在的域名报错", next(c for c in r["results"] if c["target"] == "no-such-host.invalid")["error"] != "")
    call("POST", f"/api/servers/{sid}/reality/check", {"targets": []}, expect=400)

    print("== REALITY 扫描（只扫本机回环）", flush=True)
    call("POST", f"/api/servers/{sid}/reality/scan", {"cidr": "10.0.0.0/8"}, expect=400)
    r = call("POST", f"/api/servers/{sid}/reality/scan",
             {"cidr": "127.0.0.0/29", "concurrency": 1, "max_per_second": 1})
    check("扫描开始", r["status"] == "running")
    r = call("POST", f"/api/servers/{sid}/reality/scan", {"cidr": "127.0.0.0/29"}, expect=409)
    check("同一台服务器同时只扫一个", r["code"] == "scan_running", r)
    scan = wait_until("扫描结束", lambda: (lambda s: s if s["status"] != "running" else None)(
        call("GET", f"/api/servers/{sid}/reality/scan")), timeout=120)
    check("扫描完成、回环上没有候选", scan["status"] == "done" and scan["candidates"] == [], scan)

    print("== 备份和恢复", flush=True)
    call("POST", "/api/plans", {"name": "备份前的套餐"})
    backup = call("GET", "/api/backup", raw=True)
    check("备份是 SQLite 文件", backup.startswith(b"SQLite format 3\x00"))
    call("POST", "/api/plans", {"name": "备份后的套餐"})
    call("POST", "/api/backup/restore", expect=400, data=b"not a database",
         headers={"Content-Type": "application/octet-stream"})
    r = call("POST", "/api/backup/restore", data=backup, headers={"Content-Type": "application/octet-stream"})
    check("上传备份后主控准备重启", r["restarting"] is True)
    master.wait(timeout=10)
    check("主控用退出码 75 退出", master.returncode == 75, master.returncode)
    PROCS.remove(master)
    master = start_master(args, work, args.agent_dir)
    call("POST", "/api/auth/login", {"username": "admin", "password": "ops-pass-123"})
    names = [p["name"] for p in call("GET", "/api/plans")]
    check("恢复后回到备份时的数据", names == ["备份前的套餐"], names)
    check("旧库改名保留", any(f.startswith("op-master.db.before-restore-") for f in os.listdir(f"{work}/master-data")))
    wait_server(sid, "Agent 重新连上", lambda s: s["online"])
    check("主控重启后 Agent 重新连上", True)

    print("== 删除服务器：Agent 自己卸载", flush=True)
    call("DELETE", f"/api/servers/{sid}")
    check("删除服务器成功", call("GET", "/api/servers") == [])
    wait_until("服务文件删掉", lambda: not os.path.exists("/etc/systemd/system/op-agent.service"), timeout=30)
    wait_until("服务停止", lambda: subprocess.run(["systemctl", "is-active", "op-agent"], capture_output=True,
                                                   text=True).stdout.strip() != "active", timeout=30)
    left = [p for p in ["/usr/local/bin/op-agent", "/usr/local/bin/op-agent.bak", "/etc/op-agent", "/var/lib/op-agent"]
            if os.path.exists(p)]
    check("Agent 卸载了自己的文件", left == [], left)
    nft = subprocess.run(["nft", "list", "table", "inet", NFT_TABLE], capture_output=True, text=True)
    check("没有留下 nftables 表", nft.returncode != 0)

    print(f"== 全部通过（{len(PASSED)} 项）", flush=True)


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
        cleanup_agent()
    sys.exit(code)
