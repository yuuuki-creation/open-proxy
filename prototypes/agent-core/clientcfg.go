package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
)

// 给 sing-box 命令行客户端生成配置：每个「协议 × 用户」一份，
// 各自开一个本地 SOCKS 口，测试脚本用 curl --socks5 走它。
// 这里直接拼 JSON，不用 option 结构体：客户端配置不需要进程内构造。

type ClientInfo struct {
	Protocol   string `json:"protocol"`
	User       string `json:"user"`
	ConfigPath string `json:"config_path"`
	SocksPort  int    `json:"socks_port"`
}

func writeJSON(path string, v any) error {
	data, err := json.MarshalIndent(v, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(path, append(data, '\n'), 0o644)
}

func socksInbound(port int) map[string]any {
	return map[string]any{
		"type":        "socks",
		"tag":         "socks-in",
		"listen":      "127.0.0.1",
		"listen_port": port,
	}
}

// WriteClientConfigs 为每个协议、每个用户写一份客户端配置，返回清单。
// socksBase 是本地 SOCKS 端口的起点，依次递增。
func WriteClientConfigs(outDir string, c ServerConfig, users []User, serverHost string, socksBase int) ([]ClientInfo, error) {
	var infos []ClientInfo
	port := socksBase

	for _, u := range users {
		outbounds := map[string]map[string]any{
			"vless": {
				"type":        "vless",
				"tag":         "proxy",
				"server":      serverHost,
				"server_port": int(c.portFor(0)),
				"uuid":        u.UUID,
				"flow":        "xtls-rprx-vision",
				"tls": map[string]any{
					"enabled":     true,
					"server_name": c.RealityTarget,
					"utls":        map[string]any{"enabled": true, "fingerprint": "chrome"},
					"reality": map[string]any{
						"enabled":    true,
						"public_key": c.Creds.RealityPublicKey,
						"short_id":   c.Creds.RealityShortID,
					},
				},
			},
			"hysteria2": {
				"type":        "hysteria2",
				"tag":         "proxy",
				"server":      serverHost,
				"server_port": int(c.portFor(1)),
				"password":    u.Password,
				"tls": map[string]any{
					"enabled":     true,
					"server_name": c.CertHost,
					"insecure":    true,
					"alpn":        []string{"h3"},
				},
			},
			"anytls": {
				"type":        "anytls",
				"tag":         "proxy",
				"server":      serverHost,
				"server_port": int(c.portFor(2)),
				"password":    u.Password,
				"tls": map[string]any{
					"enabled":     true,
					"server_name": c.CertHost,
					"insecure":    true,
				},
			},
			"shadowsocks": {
				"type":        "shadowsocks",
				"tag":         "proxy",
				"server":      serverHost,
				"server_port": int(c.portFor(3)),
				"method":      c.SSMethod,
				// Shadowsocks 2022 多用户：密码是「服务端主密钥:用户密钥」
				"password": c.Creds.SSServerKey + ":" + u.SSKey,
			},
		}

		for _, proto := range []string{"vless", "hysteria2", "anytls", "shadowsocks"} {
			cfg := map[string]any{
				"log":       map[string]any{"level": "warn", "timestamp": true},
				"inbounds":  []any{socksInbound(port)},
				"outbounds": []any{outbounds[proto]},
			}
			path := filepath.Join(outDir, fmt.Sprintf("client-%s-%s.json", proto, u.Name))
			if err := writeJSON(path, cfg); err != nil {
				return nil, err
			}
			infos = append(infos, ClientInfo{Protocol: proto, User: u.Name, ConfigPath: path, SocksPort: port})
			port++
		}
	}
	return infos, nil
}
