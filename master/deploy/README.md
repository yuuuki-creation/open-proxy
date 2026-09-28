# 部署主控

主控是一个静态链接的单文件 `op-master`，内置面板前端和同版本的 Agent（amd64、arm64），不依赖系统库。数据都在数据目录里（默认 `/var/lib/op-master`，SQLite 数据库 `op-master.db`）。

发布物在仓库的 Release 页：`op-master-linux-amd64`、`op-master-linux-arm64`、`SHA256SUMS`；Docker 镜像 `ghcr.io/<仓库>/op-master:<版本>`。发布流程见 `.github/workflows/release.yml`。

## 单文件 + systemd

```sh
sha256sum -c SHA256SUMS --ignore-missing
install -m 755 op-master-linux-amd64 /usr/local/bin/op-master
install -m 644 op-master.service /etc/systemd/system/op-master.service
systemctl daemon-reload
systemctl enable --now op-master
journalctl -u op-master -f
```

服务文件是同目录的 `op-master.service`。恢复备份后主控会主动退出（退出码 75），靠 `Restart=always` 重启。

## Docker

```sh
docker run -d --name op-master --restart unless-stopped \
  -p 443:443 -v op-master:/var/lib/op-master \
  ghcr.io/<仓库>/op-master:<版本>
```

一定要带重启策略，理由同上。

## 第一次打开

1. 浏览器打开 `https://<服务器 IP>/`。还没有正式证书时主控用临时自签证书，浏览器会警告，先继续访问
2. 创建管理员账号
3. 在「设置」里填主控域名和 Cloudflare API Token（权限：该域名所在区域的 DNS 编辑），主控用 DNS-01 自动申请 Let's Encrypt 证书，到期前 30 天自动续期，换证书不用重启
4. 之后用 `https://<域名>/` 访问；安装命令和订阅链接都用这个域名

## 启动参数

每个参数都可以用环境变量代替。

| 参数 | 环境变量 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `--data-dir` | `OP_MASTER_DATA_DIR` | `/var/lib/op-master` | 数据目录 |
| `--listen` | `OP_MASTER_LISTEN` | `0.0.0.0:443` | HTTPS 监听地址 |
| `--no-https` | `OP_MASTER_NO_HTTPS` | 关 | 不监听 HTTPS，放在反向代理后面时用，要同时给 `--http-listen` |
| `--http-listen` | `OP_MASTER_HTTP_LISTEN` | 无 | 另外监听的明文 HTTP 地址，例如 `127.0.0.1:8080` |
| `--public-url` | `OP_MASTER_PUBLIC_URL` | 无 | 对外地址，覆盖设置里的域名，拼安装命令和订阅链接用 |
| `--agent-dir` | `OP_MASTER_AGENT_DIR` | 无 | 从目录读 Agent 二进制和签名（`op-agent-linux-<arch>`、`.sig`），不用内置的 |
| `--acme-staging` | `OP_MASTER_ACME_STAGING` | 关 | 用 Let's Encrypt 的测试环境，只用来调试证书申请 |

## 放在反向代理后面

主控用 `--no-https --http-listen 127.0.0.1:8080 --public-url https://<域名>` 启动，由反向代理终止 TLS。要注意：

- `/api/agent/ws` 是 WebSocket，反向代理要转发 `Upgrade` 头，读超时要长于 30 秒（主控每 30 秒 ping 一次，nginx 默认的 60 秒够用）
- 反向代理要在 `X-Forwarded-For` 末尾加上客户端地址（nginx 的 `$proxy_add_x_forwarded_for`）。主控只在对端是本机时读这个头，取最后一个地址，用于登录失败限制和会话记录
- 反向代理要和主控在同一台机器上，否则主控看到的客户端地址都是反向代理的

## 升级主控

替换二进制（或换镜像版本）后重启。主控启动时自动升级数据库。Agent 和主控版本不一致时暂停同步配置、按原状态继续服务，在面板里点「全部升级」把 Agent 升到主控内置的版本。

## 备份

面板的「备份」下载整个数据库。也可以在主机上直接复制数据目录，但要先停掉主控，或者一起复制 `op-master.db-wal`。
