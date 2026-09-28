// Package reality 在节点服务器上检测和扫描 REALITY 伪装目标：要求支持 TLS 1.3 和 H2、延迟低、证书有效。
// 设计见 main 分支 nodes.md「REALITY 伪装目标」、protocol.md「REALITY 目标检测与扫描」。
// 检测是对管理员选中的几个网站做几次正常的 HTTPS 握手；扫描是按限定的并发和速率，
// 逐个连网段里各个地址的 443 端口（参考了 XTLS 的 RealiTLScanner 的思路，代码是自己写的）。
package reality

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"errors"
	"fmt"
	"net"
	"net/netip"
	"slices"
	"strconv"
	"strings"
	"sync"
	"time"

	agentv1 "github.com/yuuuki-creation/open-proxy/agent/internal/pb/openproxy/agent/v1"
)

const (
	// 一次最多检测几个目标
	maxCheckTargets = 32
	// 每个目标握手几次：几次结果不一样时（负载均衡后面的机器不同）取最保守的
	checkAttempts = 3
	// 同时检测几个目标
	checkParallel = 8
	// 整个检测的时限：主控等 1 分钟（protocol.md「消息一览」），留出回复的时间
	checkBudget = 50 * time.Second
	// 一次连接加握手的时限
	handshakeTimeout = 5 * time.Second

	// 扫描只看 443 端口
	scanPort = 443
	// 一次最多扫多少个地址（/20）
	maxScanAddresses = 4096
	// 扫描的时限：主控等 20 分钟，留出最后几个握手和回复的时间
	scanBudget = 18 * time.Minute
	// 请求里没给并发数、速率时的默认值，和上限
	defaultConcurrency = 4
	maxConcurrency     = 32
	defaultRate        = 4
	maxRate            = 50
)

// Check 检测管理员选中的伪装目标（"host" 或 "host:port"，不写端口时是 443），结果和 targets 一一对应。
// 单个目标连不上等问题写在它的结果里；只有请求本身不对时返回错误。
func Check(ctx context.Context, targets []string) ([]*agentv1.RealityTargetCheck, error) {
	if len(targets) == 0 || len(targets) > maxCheckTargets {
		return nil, fmt.Errorf("一次检测 1 到 %d 个目标，现在是 %d 个", maxCheckTargets, len(targets))
	}
	ctx, cancel := context.WithTimeout(ctx, checkBudget)
	defer cancel()
	results := make([]*agentv1.RealityTargetCheck, len(targets))
	sem := make(chan struct{}, checkParallel)
	var wg sync.WaitGroup
	for i, target := range targets {
		wg.Add(1)
		go func() {
			defer wg.Done()
			sem <- struct{}{}
			defer func() { <-sem }()
			results[i] = checkTarget(ctx, target)
		}()
	}
	wg.Wait()
	return results, nil
}

// checkTarget 对一个目标握手几次：TLS 1.3、H2、证书有效要每次成功的握手都满足才算；
// 延迟取成功几次的中位数；一次都没成功时报最后一次的错误。
func checkTarget(ctx context.Context, target string) *agentv1.RealityTargetCheck {
	result := &agentv1.RealityTargetCheck{Target: target}
	host, port, err := splitTarget(target)
	if err != nil {
		result.Error = err.Error()
		return result
	}
	ip, err := resolve(ctx, host)
	if err != nil {
		result.Error = err.Error()
		return result
	}
	var latencies []time.Duration
	var lastErr error
	tls13, h2, certValid := true, true, true
	for range checkAttempts {
		p, err := handshake(ctx, net.JoinHostPort(ip.String(), port), host)
		if err != nil {
			lastErr = err
			continue
		}
		latencies = append(latencies, p.latency)
		tls13 = tls13 && p.tls13
		h2 = h2 && p.h2
		certValid = certValid && p.certErr == nil
	}
	if len(latencies) == 0 {
		result.Error = lastErr.Error()
		return result
	}
	slices.Sort(latencies)
	result.Tls13, result.H2, result.CertificateValid = tls13, h2, certValid
	result.LatencyMs = uint32(latencies[len(latencies)/2].Milliseconds())
	return result
}

// splitTarget 把 "host" 或 "host:port" 拆开，不写端口时是 443。
func splitTarget(target string) (string, string, error) {
	target = strings.TrimSpace(target)
	host, port, err := net.SplitHostPort(target)
	if err != nil {
		// 没写端口（IPv6 地址要写成 [::1]:443）
		host, port = target, strconv.Itoa(scanPort)
	}
	if host == "" || strings.ContainsAny(host, " /:") {
		return "", "", fmt.Errorf("目标 %q 不对，应该是域名或者 域名:端口", target)
	}
	if n, err := strconv.Atoi(port); err != nil || n < 1 || n > 65535 {
		return "", "", fmt.Errorf("目标 %q 的端口不对", target)
	}
	return host, port, nil
}

// resolve 先把域名解析好，每次握手的延迟里就不含 DNS 查询；有 IPv4 地址时优先用（节点只走 IPv4）。
func resolve(ctx context.Context, host string) (netip.Addr, error) {
	if ip, err := netip.ParseAddr(host); err == nil {
		return ip, nil
	}
	ctx, cancel := context.WithTimeout(ctx, handshakeTimeout)
	defer cancel()
	ips, err := net.DefaultResolver.LookupNetIP(ctx, "ip", host)
	if err != nil {
		return netip.Addr{}, fmt.Errorf("解析 %s: %w", host, err)
	}
	for _, ip := range ips {
		if ip.Unmap().Is4() {
			return ip.Unmap(), nil
		}
	}
	if len(ips) == 0 {
		return netip.Addr{}, fmt.Errorf("解析 %s: 没有地址", host)
	}
	return ips[0], nil
}

// probe 是一次握手的结果。
type probe struct {
	tls13   bool
	h2      bool
	certErr error // 按系统根证书和主机名验证证书的结果
	leaf    *x509.Certificate
	latency time.Duration // TCP 连接加 TLS 握手的耗时（不含 DNS 查询）
}

// handshake 连 addr 做一次 TLS 握手，SNI 是 serverName（为空或是 IP 时不发 SNI）。
// 证书自己验证、不让它中断握手：验证不过也要知道 TLS 版本和 ALPN。
func handshake(ctx context.Context, addr, serverName string) (probe, error) {
	ctx, cancel := context.WithTimeout(ctx, handshakeTimeout)
	defer cancel()
	start := time.Now()
	var dialer net.Dialer
	raw, err := dialer.DialContext(ctx, "tcp", addr)
	if err != nil {
		return probe{}, fmt.Errorf("连接 %s: %w", addr, err)
	}
	defer raw.Close()
	var p probe
	conn := tls.Client(raw, &tls.Config{
		ServerName:         serverName,
		NextProtos:         []string{"h2", "http/1.1"},
		InsecureSkipVerify: true,
		VerifyConnection: func(cs tls.ConnectionState) error {
			p.certErr = verify(cs, serverName)
			return nil
		},
	})
	if err := conn.HandshakeContext(ctx); err != nil {
		return probe{}, fmt.Errorf("和 %s 的 TLS 握手: %w", addr, err)
	}
	p.latency = time.Since(start)
	state := conn.ConnectionState()
	p.tls13 = state.Version == tls.VersionTLS13
	p.h2 = state.NegotiatedProtocol == "h2"
	if len(state.PeerCertificates) > 0 {
		p.leaf = state.PeerCertificates[0]
	}
	return p, nil
}

// verify 按系统根证书和主机名验证对方的证书链。
func verify(cs tls.ConnectionState, name string) error {
	if len(cs.PeerCertificates) == 0 {
		return errors.New("对方没有给证书")
	}
	if name == "" {
		return errors.New("没有主机名，无法验证证书")
	}
	opts := x509.VerifyOptions{DNSName: name, Intermediates: x509.NewCertPool()}
	for _, c := range cs.PeerCertificates[1:] {
		opts.Intermediates.AddCert(c)
	}
	_, err := cs.PeerCertificates[0].Verify(opts)
	return err
}

// Scan 按请求里的并发数和速率，逐个连网段里各个地址的 443 端口（不发 SNI），
// 返回支持 TLS 1.3 和 H2、证书里有域名的候选，按延迟从低到高排。
func Scan(ctx context.Context, req *agentv1.ScanRealityTargets) ([]*agentv1.RealityScanCandidate, error) {
	prefix, err := netip.ParsePrefix(strings.TrimSpace(req.GetCidr()))
	if err != nil {
		return nil, fmt.Errorf("网段 %q 不对", req.GetCidr())
	}
	if !prefix.Addr().Is4() {
		return nil, errors.New("只支持扫描 IPv4 网段")
	}
	prefix = prefix.Masked()
	total := 1 << (32 - prefix.Bits())
	if total > maxScanAddresses {
		return nil, fmt.Errorf("网段 %s 有 %d 个地址，一次最多扫 %d 个", prefix, total, maxScanAddresses)
	}
	concurrency := clamp(int(req.GetConcurrency()), defaultConcurrency, maxConcurrency)
	rate := clamp(int(req.GetMaxPerSecond()), defaultRate, maxRate)
	// 最坏的情况是每个地址都等到超时，这时受并发数限制；扫不完就先拒绝，不做到一半才失败
	byRate := time.Duration(total) * time.Second / time.Duration(rate)
	worst := time.Duration(total) * handshakeTimeout / time.Duration(concurrency)
	if need := max(byRate, worst); need > scanBudget {
		return nil, fmt.Errorf("每秒 %d 个、同时 %d 个，扫 %d 个地址最长要 %v，超过了 %v", rate, concurrency, total, need.Round(time.Second), scanBudget)
	}

	ctx, cancel := context.WithTimeout(ctx, scanBudget+handshakeTimeout)
	defer cancel()
	ticker := time.NewTicker(time.Second / time.Duration(rate))
	defer ticker.Stop()
	sem := make(chan struct{}, concurrency)
	var (
		wg         sync.WaitGroup
		mu         sync.Mutex
		candidates []*agentv1.RealityScanCandidate
	)
	addr := prefix.Addr()
scan:
	for range total {
		select {
		case <-ctx.Done():
			break scan
		case <-ticker.C:
		}
		select {
		case <-ctx.Done():
			break scan
		case sem <- struct{}{}:
		}
		ip := addr
		addr = addr.Next()
		wg.Add(1)
		go func() {
			defer wg.Done()
			defer func() { <-sem }()
			if c := scanOne(ctx, ip); c != nil {
				mu.Lock()
				candidates = append(candidates, c)
				mu.Unlock()
			}
		}()
	}
	wg.Wait()
	if err := ctx.Err(); err != nil {
		return nil, fmt.Errorf("扫描没做完: %w", err)
	}
	slices.SortFunc(candidates, func(a, b *agentv1.RealityScanCandidate) int {
		return int(a.GetLatencyMs()) - int(b.GetLatencyMs())
	})
	return candidates, nil
}

// scanOne 连一个地址的 443 端口，是合格的候选就返回，否则返回 nil。
func scanOne(ctx context.Context, ip netip.Addr) *agentv1.RealityScanCandidate {
	p, err := handshake(ctx, netip.AddrPortFrom(ip, scanPort).String(), "")
	if err != nil || !p.tls13 || !p.h2 || p.leaf == nil {
		return nil
	}
	domain := certDomain(p.leaf)
	if domain == "" {
		return nil
	}
	issuer := strings.Join(p.leaf.Issuer.Organization, ", ")
	if issuer == "" {
		issuer = p.leaf.Issuer.CommonName
	}
	return &agentv1.RealityScanCandidate{
		Ip:        ip.String(),
		Domain:    domain,
		Issuer:    issuer,
		LatencyMs: uint32(p.latency.Milliseconds()),
	}
}

// certDomain 取证书里第一个不是通配符的域名；只有通配符域名时返回空（没法直接当 SNI 用）。
func certDomain(cert *x509.Certificate) string {
	for _, name := range append(slices.Clone(cert.DNSNames), cert.Subject.CommonName) {
		if name != "" && !strings.Contains(name, "*") && strings.Contains(name, ".") {
			return name
		}
	}
	return ""
}

// clamp 把请求里的值限制在 [1, upper]，0 表示用默认值。
func clamp(v, def, upper int) int {
	if v <= 0 {
		return def
	}
	return min(v, upper)
}
