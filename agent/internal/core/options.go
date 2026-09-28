package core

import (
	"crypto/tls"
	"errors"
	"fmt"
	"net/netip"
	"strconv"
	"strings"

	C "github.com/sagernet/sing-box/constant"
	"github.com/sagernet/sing-box/option"
	"github.com/sagernet/sing/common/auth"
	"github.com/sagernet/sing/common/json/badoption"

	"github.com/yuuuki-creation/open-proxy/agent/internal/mieru"
	agentv1 "github.com/yuuuki-creation/open-proxy/agent/internal/pb/openproxy/agent/v1"
	"github.com/yuuuki-creation/open-proxy/agent/internal/tracker"
)

// 期望状态到 sing-box 选项的转换。命名规则见 protocol.md「期望状态」：
// 入站 node-<节点 ID>，出站 exit-<出口 ID>，用户名是用户 ID 的十进制字符串。

// LandingTag 是自建落地 SOCKS5 入站的 tag。
const LandingTag = "landing"

func NodeTag(id uint64) string  { return tracker.NodeTagPrefix + strconv.FormatUint(id, 10) }
func ExitTag(id uint64) string  { return "exit-" + strconv.FormatUint(id, 10) }
func UserName(id uint64) string { return strconv.FormatUint(id, 10) }

// ParseNodeTag 从入站 tag 取回节点 ID；不是节点入站时返回 false。
func ParseNodeTag(tag string) (uint64, bool) {
	rest, ok := strings.CutPrefix(tag, tracker.NodeTagPrefix)
	if !ok {
		return 0, false
	}
	id, err := strconv.ParseUint(rest, 10, 64)
	return id, err == nil
}

// ParseUserName 从 sing-box 里的用户名取回用户 ID。
func ParseUserName(name string) (uint64, bool) {
	id, err := strconv.ParseUint(name, 10, 64)
	return id, err == nil
}

// 只支持 IPv4（nodes.md），入站都监听 0.0.0.0。
func listenOn(port uint32) (option.ListenOptions, error) {
	if port == 0 || port > 65535 {
		return option.ListenOptions{}, fmt.Errorf("端口 %d 不合法", port)
	}
	addr := badoption.Addr(netip.IPv4Unspecified())
	return option.ListenOptions{Listen: &addr, ListenPort: uint16(port)}, nil
}

// InboundOptions 按节点生成入站的类型和选项：sing-box 入站，或者 Mieru 节点（类型 mieru.Type）。
// users 是这个节点上放行的用户，cert 是当前可用的证书（TLS 节点用）。
func InboundOptions(node *agentv1.Node, users []*agentv1.User, cert *agentv1.Certificate) (string, any, error) {
	listen, err := listenOn(node.GetPort())
	if err != nil {
		return "", nil, err
	}
	switch p := node.GetProtocol().(type) {
	case *agentv1.Node_VlessReality:
		r := p.VlessReality
		targetPort := r.GetTargetPort()
		if targetPort == 0 {
			targetPort = 443
		}
		vlessUsers := make([]option.VLESSUser, 0, len(users))
		for _, u := range users {
			vlessUsers = append(vlessUsers, option.VLESSUser{Name: UserName(u.GetId()), UUID: u.GetUuid(), Flow: "xtls-rprx-vision"})
		}
		return C.TypeVLESS, &option.VLESSInboundOptions{
			ListenOptions: listen,
			Users:         vlessUsers,
			InboundTLSOptionsContainer: option.InboundTLSOptionsContainer{TLS: &option.InboundTLSOptions{
				Enabled:    true,
				ServerName: r.GetTargetHost(),
				Reality: &option.InboundRealityOptions{
					Enabled: true,
					Handshake: option.InboundRealityHandshakeOptions{
						ServerOptions: option.ServerOptions{Server: r.GetTargetHost(), ServerPort: uint16(targetPort)},
					},
					PrivateKey: r.GetPrivateKey(),
					ShortID:    r.GetShortIds(),
				},
			}},
		}, nil

	case *agentv1.Node_Hysteria2:
		tls, err := certificateTLS(cert, []string{"h3"})
		if err != nil {
			return "", nil, err
		}
		hy2Users := make([]option.Hysteria2User, 0, len(users))
		for _, u := range users {
			hy2Users = append(hy2Users, option.Hysteria2User{Name: UserName(u.GetId()), Password: u.GetPassword()})
		}
		options := &option.Hysteria2InboundOptions{
			ListenOptions:              listen,
			Users:                      hy2Users,
			InboundTLSOptionsContainer: option.InboundTLSOptionsContainer{TLS: tls},
		}
		if password := p.Hysteria2.GetObfsPassword(); password != "" {
			options.Obfs = &option.Hysteria2Obfs{Type: "salamander", Password: password}
		}
		return C.TypeHysteria2, options, nil

	case *agentv1.Node_Anytls:
		tls, err := certificateTLS(cert, nil)
		if err != nil {
			return "", nil, err
		}
		anytlsUsers := make([]option.AnyTLSUser, 0, len(users))
		for _, u := range users {
			anytlsUsers = append(anytlsUsers, option.AnyTLSUser{Name: UserName(u.GetId()), Password: u.GetPassword()})
		}
		return C.TypeAnyTLS, &option.AnyTLSInboundOptions{
			ListenOptions:              listen,
			Users:                      anytlsUsers,
			InboundTLSOptionsContainer: option.InboundTLSOptionsContainer{TLS: tls},
		}, nil

	case *agentv1.Node_Shadowsocks2022:
		ss := p.Shadowsocks2022
		ssUsers := make([]option.ShadowsocksUser, 0, len(users))
		for _, u := range users {
			ssUsers = append(ssUsers, option.ShadowsocksUser{Name: UserName(u.GetId()), Password: u.GetSsKey()})
		}
		// 不开 managed：那会禁止配置里的静态用户列表，而不开也能热更新用户（原型 V4）
		return C.TypeShadowsocks, &option.ShadowsocksInboundOptions{
			ListenOptions: listen,
			Method:        ss.GetMethod(),
			Password:      ss.GetServerKey(),
			Users:         ssUsers,
		}, nil

	case *agentv1.Node_Mieru:
		// 不是 sing-box 的入站，由 internal/mieru 运行，连接交给 sing-box 路由
		mieruUsers := make([]mieru.User, 0, len(users))
		for _, u := range users {
			mieruUsers = append(mieruUsers, mieru.User{Name: UserName(u.GetId()), Password: u.GetPassword()})
		}
		return mieru.Type, &mieru.Options{Port: listen.ListenPort, Users: mieruUsers}, nil
	}
	return "", nil, errors.New("节点没有协议参数，或者是不认识的协议")
}

// CheckCertificate 校验证书和私钥：和 sing-box 构造 TLS 入站时一样用 tls.X509KeyPair 解析。
// 证书是期望状态里单独的一项，先单独校验，不通过时 TLS 节点接着用原来的证书。
func CheckCertificate(cert *agentv1.Certificate) error {
	if cert.GetCertPem() == "" || cert.GetKeyPem() == "" {
		return errors.New("证书或私钥是空的")
	}
	if _, err := tls.X509KeyPair([]byte(cert.GetCertPem()), []byte(cert.GetKeyPem())); err != nil {
		return fmt.Errorf("解析证书和私钥: %w", err)
	}
	return nil
}

// certificateTLS 生成用当前证书的 TLS 选项（Hysteria2、AnyTLS 用）。
func certificateTLS(cert *agentv1.Certificate, alpn []string) (*option.InboundTLSOptions, error) {
	if cert.GetCertPem() == "" || cert.GetKeyPem() == "" {
		return nil, errors.New("没有校验通过的证书")
	}
	return &option.InboundTLSOptions{
		Enabled:     true,
		ALPN:        alpn,
		Certificate: badoption.Listable[string]{cert.GetCertPem()},
		Key:         badoption.Listable[string]{cert.GetKeyPem()},
	}, nil
}

// ExitOptions 生成落地出口（SOCKS5 出站）的类型和选项。
func ExitOptions(exit *agentv1.Exit) (string, any, error) {
	if exit.GetHost() == "" || exit.GetPort() == 0 || exit.GetPort() > 65535 {
		return "", nil, fmt.Errorf("落地出口地址不完整：%s:%d", exit.GetHost(), exit.GetPort())
	}
	return C.TypeSOCKS, &option.SOCKSOutboundOptions{
		ServerOptions: option.ServerOptions{Server: exit.GetHost(), ServerPort: uint16(exit.GetPort())},
		Version:       "5",
		Username:      exit.GetUsername(),
		Password:      exit.GetPassword(),
	}, nil
}

// LandingOptions 生成自建落地的 SOCKS5 入站。来源 IP 的放行在分发出站里检查。
func LandingOptions(landing *agentv1.Landing) (string, any, error) {
	listen, err := listenOn(landing.GetPort())
	if err != nil {
		return "", nil, err
	}
	if landing.GetUsername() == "" || landing.GetPassword() == "" {
		return "", nil, errors.New("自建落地缺少账号或密码")
	}
	return C.TypeSOCKS, &option.SocksInboundOptions{
		ListenOptions: listen,
		Users:         []auth.User{{Username: landing.GetUsername(), Password: landing.GetPassword()}},
	}, nil
}
