#!/usr/bin/env python3
"""主控运维功能的端到端测试：安装脚本、二进制下载的认证、用安装命令装 Agent（systemd 服务）、
删除服务器时卸载、备份和恢复。

用法（在测试 VPS 上用 root 跑，整个脚本放进 open-proxy.slice）：
  python3 e2e_ops.py --master <op-master> --agent-dir <目录：op-agent-linux-amd64 和 .sig> --workdir <空目录>

会在系统里装 op-agent 服务（systemd 单元带 Slice=open-proxy.slice），结束时如果还在就清理掉。
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request

MASTER = "http://127.0.0.1:28080"
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


def start_master(args, work):
    log = open(os.path.join(work, "master.log"), "ab")
    proc = subprocess.Popen([args.master, "--data-dir", f"{work}/master-data", "--no-https",
                             "--http-listen", "127.0.0.1:28080", "--public-url", MASTER,
                             "--agent-dir", args.agent_dir], stdout=log, stderr=subprocess.STDOUT)
    PROCS.append(proc)
    wait_until("主控启动", lambda: call("GET", "/api/health"), timeout=15)
    return proc


def cleanup_agent():
    subprocess.run(["systemctl", "disable", "--now", "op-agent"], capture_output=True)
    for path in ["/etc/systemd/system/op-agent.service", "/usr/local/bin/op-agent",
                 "/usr/local/bin/op-agent.bak", "/etc/op-agent/op-agent.conf"]:
        if os.path.exists(path):
            os.remove(path)
    subprocess.run(["rm", "-rf", "/var/lib/op-agent", "/etc/op-agent"])
    subprocess.run(["systemctl", "daemon-reload"])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--master", required=True)
    ap.add_argument("--agent-dir", required=True)
    ap.add_argument("--workdir", required=True)
    args = ap.parse_args()
    work = os.path.abspath(args.workdir)
    os.makedirs(work, exist_ok=True)

    print("== 启动主控", flush=True)
    master = start_master(args, work)
    call("POST", "/api/setup", {"username": "admin", "password": "ops-pass-123"})
    server = call("POST", "/api/servers", {"name": "本机", "address": "127.0.0.1",
                                           "port_range_start": 21000, "port_range_end": 21999})
    sid = server["server"]["id"]
    token = server["install_command"].split()[-1]

    print("== 安装脚本和二进制下载", flush=True)
    binary = open(os.path.join(args.agent_dir, "op-agent-linux-amd64"), "rb").read()
    version = call("GET", "/api/health")["version"]
    script = call("GET", "/api/agent/install.sh", raw=True).decode()
    check("安装脚本填好了版本和 SHA-256", f'VERSION="{version}"' in script
          and f'SHA256_AMD64="{hashlib.sha256(binary).hexdigest()}"' in script and "__" not in script)
    call("GET", f"/api/agent/binary/{version}/amd64", expect=401, raw=True)
    call("GET", f"/api/agent/binary/{version}/amd64", expect=401, raw=True,
         headers={"Authorization": "Bearer wrong-token"})
    call("GET", "/api/agent/binary/0.0.1/amd64", expect=404, raw=True,
         headers={"Authorization": f"Bearer {token}"})
    got = call("GET", f"/api/agent/binary/{version}/amd64", raw=True, headers={"Authorization": f"Bearer {token}"})
    check("带 Token 下载到的二进制和原文件一致", hashlib.sha256(got).digest() == hashlib.sha256(binary).digest())

    print("== 用安装命令装 Agent", flush=True)
    cleanup_agent()
    cmd = server["install_command"]
    result = subprocess.run(["bash", "-c", cmd.replace("| bash -s --", "| OP_AGENT_SLICE=open-proxy.slice bash -s --")],
                            capture_output=True, text=True)
    print("    " + (result.stdout + result.stderr).strip().replace("\n", "\n    "), flush=True)
    check("安装命令执行成功", result.returncode == 0)
    unit = open("/etc/systemd/system/op-agent.service").read()
    check("systemd 服务在 open-proxy.slice 里", "Slice=open-proxy.slice" in unit)
    check("配置文件权限 0600", oct(os.stat("/etc/op-agent/op-agent.conf").st_mode & 0o777) == "0o600")
    s = wait_until("Agent 上线", lambda: (lambda s: s if s["online"] else None)(call("GET", f"/api/servers/{sid}")))
    check("装好的 Agent 连上了主控", s["online"] and s["agent_arch"] == "amd64")

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
    master = start_master(args, work)
    call("POST", "/api/auth/login", {"username": "admin", "password": "ops-pass-123"})
    names = [p["name"] for p in call("GET", "/api/plans")]
    check("恢复后回到备份时的数据", names == ["备份前的套餐"], names)
    check("旧库改名保留", any(f.startswith("op-master.db.before-restore-") for f in os.listdir(f"{work}/master-data")))
    wait_until("Agent 重新连上", lambda: call("GET", f"/api/servers/{sid}")["online"], timeout=60)
    check("主控重启后 Agent 重新连上", True)

    print("== 删除服务器", flush=True)
    call("DELETE", f"/api/servers/{sid}")
    time.sleep(8)
    active = subprocess.run(["systemctl", "is-active", "op-agent"], capture_output=True, text=True).stdout.strip()
    print(f"    删除后 op-agent 服务状态：{active}（Agent 实现卸载（M7）之前会一直重连，收到「服务器已删除」）", flush=True)
    check("删除服务器成功", call("GET", "/api/servers") == [])

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
