#!/usr/bin/env python3
"""主控管理接口的冒烟测试：从首次初始化开始，把管理接口都走一遍，检查返回值和错误码。

用法：python3 api_smoke.py http://127.0.0.1:28080
主控要用一个空的数据目录启动，并用 --public-url 指定同一个地址。只用 Python 标准库。
"""

import json
import sys
import urllib.error
import urllib.request

BASE = sys.argv[1].rstrip("/") if len(sys.argv) > 1 else "http://127.0.0.1:28080"
COOKIE = {"value": ""}
PASSED = []


def call(method, path, body=None, expect=200, auth=True):
    """发请求，检查状态码，返回解析后的 JSON。Cookie 自己管：标准库不会在 http:// 上发 Secure Cookie。"""
    data = json.dumps(body if body is not None else {}).encode() if method != "GET" else None
    req = urllib.request.Request(BASE + path, data=data, method=method)
    if method != "GET":
        req.add_header("Content-Type", "application/json")
    if auth and COOKIE["value"]:
        req.add_header("Cookie", COOKIE["value"])
    try:
        with urllib.request.urlopen(req) as resp:
            status, headers, raw = resp.status, resp.headers, resp.read()
    except urllib.error.HTTPError as err:
        status, headers, raw = err.code, err.headers, err.read()
    for value in headers.get_all("Set-Cookie") or []:
        pair = value.split(";", 1)[0]
        if pair.startswith("op_session="):
            COOKIE["value"] = pair if pair != "op_session=" else ""
    payload = json.loads(raw) if raw else None
    if status != expect:
        raise AssertionError(f"{method} {path}：期望 {expect}，实际 {status}，返回 {payload}")
    return payload


def check(name, cond, detail=""):
    if not cond:
        raise AssertionError(f"{name} 不成立 {detail}")
    PASSED.append(name)
    print(f"  ✓ {name}")


def main():
    print(f"== 主控接口冒烟测试：{BASE}")

    # 初始化和登录
    check("初始化前 initialized=false", call("GET", "/api/setup")["initialized"] is False)
    call("GET", "/api/servers", expect=401)
    call("POST", "/api/setup", {"username": "admin", "password": "short"}, expect=400)
    r = call("POST", "/api/setup", {"username": "admin", "password": "test-pass-123", "timezone": "Asia/Shanghai"})
    check("初始化后自动登录", r["username"] == "admin" and COOKIE["value"])
    call("POST", "/api/setup", {"username": "x", "password": "test-pass-123"}, expect=403)
    check("me", call("GET", "/api/auth/me")["username"] == "admin")
    s = call("GET", "/api/settings")
    check("设置：时区、公开地址", s["timezone"] == "Asia/Shanghai" and s["public_url"] == BASE)
    call("PATCH", "/api/settings", {"timezone": "Mars/Base"}, expect=400)

    # 写操作必须是 JSON
    req = urllib.request.Request(BASE + "/api/plans", data=b"name=x", method="POST")
    req.add_header("Content-Type", "application/x-www-form-urlencoded")
    req.add_header("Cookie", COOKIE["value"])
    try:
        urllib.request.urlopen(req)
        check("表单格式的写操作被拒绝", False)
    except urllib.error.HTTPError as err:
        check("表单格式的写操作被拒绝", err.code == 415, f"实际 {err.code}")

    # 服务器
    r = call("POST", "/api/servers", {"name": "测试机", "address": "203.0.113.10",
                                      "port_range_start": 20000, "port_range_end": 29999})
    server = r["server"]
    check("创建服务器返回安装命令", r["install_command"].startswith(f"curl -fsSL {BASE}/api/agent/install.sh"))
    check("自签证书已生成", server["certificate"] and server["certificate"]["kind"] == "self_signed"
          and len(server["certificate"]["sha256"]) == 64)
    call("POST", "/api/servers", {"name": "测试机", "address": "1.2.3.4"}, expect=409)
    call("POST", "/api/servers", {"name": "v6", "address": "2001:db8::1"}, expect=400)
    r = call("POST", f"/api/servers/{server['id']}/install-command")
    check("重新生成安装命令", "install.sh" in r["install_command"])
    sid = server["id"]

    # 节点：五种协议
    vless = call("POST", "/api/nodes", {"server_id": sid, "name": "VLESS", "protocol": "vless_reality",
                                        "reality_target": "www.microsoft.com"})
    check("VLESS：自动分配端口、生成密钥", 20000 <= vless["port"] <= 29999
          and len(vless["reality"]["public_key"]) == 43 and vless["reality"]["target"] == "www.microsoft.com:443")
    call("POST", "/api/nodes", {"server_id": sid, "name": "x", "protocol": "vless_reality"}, expect=400)
    hy2 = call("POST", "/api/nodes", {"server_id": sid, "name": "Hy2", "protocol": "hysteria2", "obfs": True,
                                      "hop_ports": {"start": 30000, "end": 30099}})
    check("Hysteria2：混淆和端口跳跃", hy2["obfs"] and hy2["hop_ports"] == {"start": 30000, "end": 30099})
    anytls = call("POST", "/api/nodes", {"server_id": sid, "name": "AnyTLS", "protocol": "anytls", "port": 24443})
    check("AnyTLS：手动端口", anytls["port"] == 24443)
    call("POST", "/api/nodes", {"server_id": sid, "name": "dup", "protocol": "anytls", "port": 24443}, expect=409)
    call("POST", "/api/nodes", {"server_id": sid, "name": "hop", "protocol": "anytls", "port": 30050}, expect=409)
    ss = call("POST", "/api/nodes", {"server_id": sid, "name": "SS", "protocol": "shadowsocks2022"})
    mieru = call("POST", "/api/nodes", {"server_id": sid, "name": "Mieru", "protocol": "mieru"})
    check("节点列表", len(call("GET", "/api/nodes")) == 5)
    r = call("PATCH", f"/api/nodes/{hy2['id']}", {"obfs": False, "hop_ports": None})
    check("修改节点：关混淆和端口跳跃", not r["obfs"] and r["hop_ports"] is None)
    call("PATCH", f"/api/nodes/{anytls['id']}", {"port": vless["port"]}, expect=409)
    ids = [mieru["id"], ss["id"], anytls["id"], hy2["id"], vless["id"]]
    call("PUT", "/api/nodes/order", {"ids": ids})
    check("调整顺序", [n["id"] for n in call("GET", "/api/nodes")] == ids)

    # 落地出口
    third = call("POST", "/api/exits", {"name": "第三方", "kind": "third_party", "host": "203.0.113.9",
                                        "port": 1080, "username": "u", "password": "p"})
    landing = call("POST", "/api/exits", {"name": "自建", "kind": "self_built", "landing_server_id": sid})
    check("自建出口：生成账号密码、用落地机地址", landing["host"] == "203.0.113.10"
          and len(landing["password"]) == 24 and 20000 <= landing["port"] <= 29999)
    call("PATCH", f"/api/nodes/{ss['id']}", {"exit_id": third["id"]})
    call("DELETE", f"/api/exits/{third['id']}", expect=409)
    call("DELETE", f"/api/servers/{sid}", expect=409)
    call("PATCH", f"/api/nodes/{ss['id']}", {"exit_id": None})
    call("DELETE", f"/api/exits/{third['id']}")
    check("删除没人用的出口", len(call("GET", "/api/exits")) == 1)

    # 套餐和用户
    plan = call("POST", "/api/plans", {"name": "标准", "traffic_quota_bytes": 100 * 2**30,
                                       "node_ids": [vless["id"], ss["id"], vless["id"]]})
    check("套餐：节点去重", plan["node_ids"] == sorted([vless["id"], ss["id"]]))
    call("POST", "/api/plans", {"name": "x", "node_ids": [999999]}, expect=400)
    user = call("POST", "/api/users", {"name": "小明", "plan_id": plan["id"], "expires_on": "2027-01-31"})
    check("用户：开通日默认今天、订阅链接", len(user["started_on"]) == 10 and user["sub_url"].startswith(BASE + "/s/"))
    call("DELETE", f"/api/plans/{plan['id']}", expect=409)
    r = call("PATCH", f"/api/users/{user['id']}", {"enabled": False, "expires_on": None, "remark": "测试"})
    check("修改用户", r["enabled"] is False and r["expires_on"] is None and r["remark"] == "测试")
    call("POST", f"/api/users/{user['id']}/reset-period")
    r = call("POST", f"/api/users/{user['id']}/reset-credentials")
    check("重置凭据后订阅链接变了", r["sub_url"] != user["sub_url"])
    check("套餐列表带绑定人数", call("GET", f"/api/plans/{plan['id']}")["user_count"] == 1)

    # 删除
    call("DELETE", f"/api/users/{user['id']}")
    call("DELETE", f"/api/plans/{plan['id']}")
    call("DELETE", f"/api/exits/{landing['id']}")
    call("DELETE", f"/api/servers/{sid}")
    check("删除服务器后节点一起删", call("GET", "/api/nodes") == [])

    # 改密码后要重新登录；登录失败限流
    call("PUT", "/api/auth/password", {"old_password": "wrong-pass", "new_password": "new-pass-456"}, expect=400)
    call("PUT", "/api/auth/password", {"old_password": "test-pass-123", "new_password": "new-pass-456"})
    call("GET", "/api/auth/me", expect=401)
    call("POST", "/api/auth/login", {"username": "admin", "password": "new-pass-456"})
    check("新密码登录", call("GET", "/api/auth/me")["username"] == "admin")
    call("POST", "/api/auth/logout")
    call("GET", "/api/auth/me", expect=401)
    for _ in range(5):
        call("POST", "/api/auth/login", {"username": "admin", "password": "bad"}, expect=401)
    call("POST", "/api/auth/login", {"username": "admin", "password": "new-pass-456"}, expect=429)
    check("连续失败 5 次后锁定", True)

    print(f"== 全部通过（{len(PASSED)} 项）")


if __name__ == "__main__":
    try:
        main()
    except AssertionError as err:
        print(f"✗ {err}")
        sys.exit(1)
