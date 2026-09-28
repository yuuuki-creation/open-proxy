package main

import (
	"errors"
	"fmt"
	"log/slog"
	"net"
	"net/url"
	"os"
	"strings"
	"unicode"
)

// Config 是安装脚本写的配置文件 /etc/op-agent/op-agent.conf 的内容。
// 格式：每行一个 KEY=value，空行和 # 开头的行忽略。Agent 只读不写，
// 所以不会出现「写出自己读不回来的值」的问题。见 architecture.md「发布、安装与升级」。
type Config struct {
	// 主控地址，例如 https://panel.example.com，不带路径。
	// 只有本机回环地址可以用 http://（主控和 Agent 装在同一台机器上测试时用）。
	MasterURL string
	// 这台服务器的 Agent Token。
	Token string
}

func loadConfig(path string) (*Config, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("读取配置文件: %w", err)
	}
	cfg, err := parseConfig(string(data))
	if err != nil {
		return nil, fmt.Errorf("配置文件 %s: %w", path, err)
	}
	return cfg, nil
}

func parseConfig(text string) (*Config, error) {
	cfg := &Config{}
	for i, line := range strings.Split(text, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		key, value, ok := strings.Cut(line, "=")
		if !ok {
			return nil, fmt.Errorf("第 %d 行缺少「=」", i+1)
		}
		key = strings.TrimSpace(key)
		value = strings.TrimSpace(value)
		switch key {
		case "MASTER_URL":
			cfg.MasterURL = value
		case "TOKEN":
			cfg.Token = value
		default:
			// 不认识的键只警告：新版安装脚本加了键，旧版 Agent 也能启动
			slog.Warn("配置文件里有不认识的键，已忽略", "line", i+1, "key", key)
		}
	}
	if err := cfg.validate(); err != nil {
		return nil, err
	}
	return cfg, nil
}

func (c *Config) validate() error {
	u, err := url.Parse(c.MasterURL)
	if err != nil || u.Host == "" {
		return fmt.Errorf("MASTER_URL 不是合法的地址，现在是 %q", c.MasterURL)
	}
	// Token 在第一条消息里明文发送，所以除了不出本机的回环地址，一律要求 HTTPS
	secure := u.Scheme == "https" || (u.Scheme == "http" && isLoopback(u.Hostname()))
	if !secure {
		return fmt.Errorf("MASTER_URL 必须是 https:// 开头的地址（只有本机回环地址可以用 http://），现在是 %q", c.MasterURL)
	}
	if strings.Trim(u.Path, "/") != "" || u.RawQuery != "" {
		return fmt.Errorf("MASTER_URL 只写到域名，不带路径和参数，现在是 %q", c.MasterURL)
	}
	c.MasterURL = u.Scheme + "://" + u.Host

	if c.Token == "" {
		return errors.New("缺少 TOKEN")
	}
	for _, r := range c.Token {
		if r > unicode.MaxASCII || !unicode.IsPrint(r) || unicode.IsSpace(r) {
			return errors.New("TOKEN 里有空白、控制字符或非 ASCII 字符")
		}
	}
	return nil
}

func isLoopback(host string) bool {
	if host == "localhost" {
		return true
	}
	ip := net.ParseIP(host)
	return ip != nil && ip.IsLoopback()
}
