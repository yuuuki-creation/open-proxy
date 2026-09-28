// mieruclient 是测试用的 Mieru 客户端，只用于在测试 VPS 上验证 op-agent 的 Mieru 节点，不随 Agent 发布。
//
// 用 mieru 的 apis/client 连 Agent 的 Mieru 节点，按参数做一件事，每次的结果打一行：
//
//	-http URL             经节点访问一个 http:// 地址（CONNECT），打印状态行和字节数
//	-dns IP:端口 -name 域名  经节点发一个 DNS 查询（UDP ASSOCIATE），打印回答数
//	-echo IP:端口          经节点连一个回显服务，一条连接上每隔 -interval 收发一次，看节点变更时连接断没断
//	-echo-server IP:端口   不连节点，自己当回显服务
//
// -count 是做几次（-echo 是收发几次）。
package main

import (
	"bufio"
	"context"
	"encoding/binary"
	"errors"
	"flag"
	"fmt"
	"io"
	"log/slog"
	"net"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/enfein/mieru/v3/apis/client"
	apicommon "github.com/enfein/mieru/v3/apis/common"
	"github.com/enfein/mieru/v3/apis/model"
	"github.com/enfein/mieru/v3/pkg/appctl/appctlpb"
	"google.golang.org/protobuf/proto"
)

func main() {
	server := flag.String("server", "127.0.0.1:21001", "Mieru 节点的地址")
	user := flag.String("user", "", "用户名（用户 ID）")
	password := flag.String("password", "", "密码")
	httpURL := flag.String("http", "", "经节点访问的 http:// 地址")
	dnsServer := flag.String("dns", "", "经节点查询的 DNS 服务器，IP:端口")
	name := flag.String("name", "example.com", "DNS 查询的域名")
	echo := flag.String("echo", "", "经节点连的回显服务，IP:端口")
	echoServer := flag.String("echo-server", "", "自己当回显服务，监听这个地址")
	count := flag.Int("count", 1, "做几次")
	interval := flag.Duration("interval", time.Second, "每次之间隔多久")
	flag.Parse()
	slog.SetDefault(slog.New(slog.NewTextHandler(os.Stderr, nil)))

	if *echoServer != "" {
		// 只在出错时返回
		slog.Error("回显服务退出", "err", serveEcho(*echoServer))
		os.Exit(1)
	}

	c, err := newClient(*server, *user, *password)
	if err != nil {
		slog.Error("创建 Mieru 客户端", "err", err)
		os.Exit(1)
	}
	defer c.Stop()

	failed := 0
	switch {
	case *echo != "":
		failed = runEcho(c, *echo, *count, *interval)
	default:
		for i := 1; i <= *count; i++ {
			var result string
			var err error
			if *httpURL != "" {
				result, err = fetch(c, *httpURL)
			} else if *dnsServer != "" {
				result, err = query(c, *dnsServer, *name)
			} else {
				err = errors.New("没有指定 -http、-dns 或 -echo")
			}
			report(i, result, err)
			if err != nil {
				failed++
			}
			if i < *count {
				time.Sleep(*interval)
			}
		}
	}
	fmt.Printf("结束：失败 %d 次\n", failed)
	if failed > 0 {
		os.Exit(1)
	}
}

func report(i int, result string, err error) {
	now := time.Now().Format("15:04:05.000")
	if err != nil {
		fmt.Printf("%s 第 %d 次 失败 %v\n", now, i, err)
		return
	}
	fmt.Printf("%s 第 %d 次 成功 %s\n", now, i, result)
}

func newClient(server, user, password string) (client.Client, error) {
	host, portStr, err := net.SplitHostPort(server)
	if err != nil {
		return nil, err
	}
	port, err := strconv.Atoi(portStr)
	if err != nil {
		return nil, err
	}
	c := client.NewClient()
	err = c.Store(&client.ClientConfig{Profile: &appctlpb.ClientProfile{
		ProfileName: proto.String("test"),
		User:        &appctlpb.User{Name: proto.String(user), Password: proto.String(password)},
		Servers: []*appctlpb.ServerEndpoint{{
			IpAddress:    proto.String(host),
			PortBindings: []*appctlpb.PortBinding{{Port: proto.Int32(int32(port)), Protocol: appctlpb.TransportProtocol_TCP.Enum()}},
		}},
	}})
	if err != nil {
		return nil, err
	}
	if err := c.Start(); err != nil {
		return nil, err
	}
	return c, nil
}

// dial 经节点连目标，network 是 tcp 或 udp。
func dial(c client.Client, network, target string) (net.Conn, error) {
	host, portStr, err := net.SplitHostPort(target)
	if err != nil {
		return nil, err
	}
	port, err := strconv.Atoi(portStr)
	if err != nil {
		return nil, err
	}
	addr := model.NetAddrSpec{Net: network, AddrSpec: model.AddrSpec{Port: port}}
	if ip := net.ParseIP(host); ip != nil {
		addr.IP = ip
	} else {
		addr.FQDN = host
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	return c.DialContext(ctx, addr)
}

// fetch 经节点发一个 HTTP GET，返回状态行和响应体字节数。
func fetch(c client.Client, rawURL string) (string, error) {
	u, err := url.Parse(rawURL)
	if err != nil || u.Scheme != "http" {
		return "", fmt.Errorf("只支持 http:// 地址：%q", rawURL)
	}
	target := u.Host
	if u.Port() == "" {
		target = net.JoinHostPort(u.Hostname(), "80")
	}
	conn, err := dial(c, "tcp", target)
	if err != nil {
		return "", fmt.Errorf("连接: %w", err)
	}
	defer conn.Close()
	conn.SetDeadline(time.Now().Add(30 * time.Second))
	req, err := http.NewRequest(http.MethodGet, rawURL, nil)
	if err != nil {
		return "", err
	}
	req.Close = true
	if err := req.Write(conn); err != nil {
		return "", fmt.Errorf("发请求: %w", err)
	}
	resp, err := http.ReadResponse(bufio.NewReader(conn), req)
	if err != nil {
		return "", fmt.Errorf("读响应: %w", err)
	}
	defer resp.Body.Close()
	n, err := io.Copy(io.Discard, resp.Body)
	if err != nil {
		return "", fmt.Errorf("读响应体: %w", err)
	}
	return fmt.Sprintf("%s，响应体 %d 字节", resp.Status, n), nil
}

// query 经节点的 UDP 关联发一个 A 记录查询，返回回答数。
func query(c client.Client, server, name string) (string, error) {
	addr, err := net.ResolveUDPAddr("udp", server)
	if err != nil {
		return "", err
	}
	conn, err := dial(c, "udp", server)
	if err != nil {
		return "", fmt.Errorf("建立 UDP 关联: %w", err)
	}
	defer conn.Close()
	conn.SetDeadline(time.Now().Add(15 * time.Second))
	// mihomo 用 mieru 的客户端 API 时也是这样包两层
	pc := apicommon.NewUDPAssociateWrapper(apicommon.NewPacketOverStreamTunnel(conn))
	id := uint16(time.Now().UnixNano())
	if _, err := pc.WriteTo(dnsQuery(id, name), addr); err != nil {
		return "", fmt.Errorf("发查询: %w", err)
	}
	buf := make([]byte, 4096)
	n, from, err := pc.ReadFrom(buf)
	if err != nil {
		return "", fmt.Errorf("收回答: %w", err)
	}
	if n < 12 || binary.BigEndian.Uint16(buf) != id {
		return "", fmt.Errorf("回答不对：%d 字节", n)
	}
	return fmt.Sprintf("来自 %v，回答 %d 条，%d 字节", from, binary.BigEndian.Uint16(buf[6:]), n), nil
}

// dnsQuery 拼一个递归查询 A 记录的 DNS 请求。
func dnsQuery(id uint16, name string) []byte {
	msg := make([]byte, 12, 64)
	binary.BigEndian.PutUint16(msg[0:], id)
	binary.BigEndian.PutUint16(msg[2:], 0x0100) // RD
	binary.BigEndian.PutUint16(msg[4:], 1)      // QDCOUNT
	for _, label := range strings.Split(strings.TrimSuffix(name, "."), ".") {
		msg = append(msg, byte(len(label)))
		msg = append(msg, label...)
	}
	msg = append(msg, 0, 0, 1, 0, 1) // 根、QTYPE A、QCLASS IN
	return msg
}

// runEcho 经节点连回显服务，在同一条连接上收发 count 次，返回失败次数。连接断了就重连。
func runEcho(c client.Client, target string, count int, interval time.Duration) int {
	failed := 0
	var conn net.Conn
	var reader *bufio.Reader
	for i := 1; i <= count; i++ {
		if conn == nil {
			var err error
			conn, err = dial(c, "tcp", target)
			if err != nil {
				report(i, "", fmt.Errorf("连接: %w", err))
				failed++
				time.Sleep(interval)
				continue
			}
			reader = bufio.NewReader(conn)
			fmt.Printf("%s 新建连接\n", time.Now().Format("15:04:05.000"))
		}
		line := fmt.Sprintf("ping %d\n", i)
		conn.SetDeadline(time.Now().Add(10 * time.Second))
		_, err := conn.Write([]byte(line))
		var back string
		if err == nil {
			back, err = reader.ReadString('\n')
		}
		if err == nil && back != line {
			err = fmt.Errorf("收到的不对：%q", back)
		}
		report(i, strings.TrimSpace(back), err)
		if err != nil {
			failed++
			conn.Close()
			conn = nil
		}
		if i < count {
			time.Sleep(interval)
		}
	}
	if conn != nil {
		conn.Close()
	}
	return failed
}

func serveEcho(address string) error {
	l, err := net.Listen("tcp", address)
	if err != nil {
		return err
	}
	slog.Info("回显服务启动", "listen", address)
	for {
		conn, err := l.Accept()
		if err != nil {
			return err
		}
		go func() {
			defer conn.Close()
			io.Copy(conn, conn)
		}()
	}
}
