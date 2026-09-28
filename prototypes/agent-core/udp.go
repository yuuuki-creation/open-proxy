package main

import (
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"net"
	"time"
)

// UDP 回显服务：原样把收到的数据发回去，用来验证 UDP 方向的计量。
func StartUDPEcho(addr string) (func(), error) {
	pc, err := net.ListenPacket("udp", addr)
	if err != nil {
		return nil, err
	}
	go func() {
		buf := make([]byte, 64*1024)
		for {
			n, peer, err := pc.ReadFrom(buf)
			if err != nil {
				return
			}
			pc.WriteTo(buf[:n], peer)
		}
	}()
	return func() { pc.Close() }, nil
}

// UDPTestResult 是一次 UDP 测试的结果。
type UDPTestResult struct {
	Sent       int   `json:"sent"`
	Received   int   `json:"received"`
	BytesUp    int64 `json:"bytes_up"`
	BytesDown  int64 `json:"bytes_down"`
	LostPacket int   `json:"lost"`
}

// RunUDPTest 通过 SOCKS5 的 UDP ASSOCIATE 发若干个包到回显服务，统计收发字节。
// 这样才能测到 UDP 方向的计量：curl 只会走 TCP。
func RunUDPTest(socksAddr, target string, packets, size int, timeout time.Duration) (*UDPTestResult, error) {
	targetHost, targetPortStr, err := net.SplitHostPort(target)
	if err != nil {
		return nil, err
	}
	targetIP := net.ParseIP(targetHost)
	if targetIP == nil || targetIP.To4() == nil {
		return nil, fmt.Errorf("目标必须是 IPv4 地址: %s", target)
	}
	var targetPort int
	fmt.Sscanf(targetPortStr, "%d", &targetPort)

	// 1. SOCKS5 握手（无认证）
	ctrl, err := net.DialTimeout("tcp", socksAddr, timeout)
	if err != nil {
		return nil, err
	}
	defer ctrl.Close()
	ctrl.SetDeadline(time.Now().Add(timeout))
	if _, err = ctrl.Write([]byte{0x05, 0x01, 0x00}); err != nil {
		return nil, err
	}
	resp := make([]byte, 2)
	if _, err = io.ReadFull(ctrl, resp); err != nil {
		return nil, err
	}
	if resp[0] != 0x05 || resp[1] != 0x00 {
		return nil, errors.New("SOCKS5 握手失败")
	}

	// 2. UDP ASSOCIATE
	req := []byte{0x05, 0x03, 0x00, 0x01, 0, 0, 0, 0, 0, 0}
	if _, err = ctrl.Write(req); err != nil {
		return nil, err
	}
	head := make([]byte, 4)
	if _, err = io.ReadFull(ctrl, head); err != nil {
		return nil, err
	}
	if head[1] != 0x00 {
		return nil, fmt.Errorf("UDP ASSOCIATE 被拒绝，代码 %d", head[1])
	}
	var relayIP net.IP
	switch head[3] {
	case 0x01:
		b := make([]byte, 4)
		if _, err = io.ReadFull(ctrl, b); err != nil {
			return nil, err
		}
		relayIP = net.IP(b)
	case 0x04:
		b := make([]byte, 16)
		if _, err = io.ReadFull(ctrl, b); err != nil {
			return nil, err
		}
		relayIP = net.IP(b)
	default:
		return nil, fmt.Errorf("不支持的地址类型 %d", head[3])
	}
	portBytes := make([]byte, 2)
	if _, err = io.ReadFull(ctrl, portBytes); err != nil {
		return nil, err
	}
	relayPort := binary.BigEndian.Uint16(portBytes)
	if relayIP.IsUnspecified() {
		host, _, _ := net.SplitHostPort(socksAddr)
		relayIP = net.ParseIP(host)
	}

	// 3. 发包并读回显
	conn, err := net.DialUDP("udp", nil, &net.UDPAddr{IP: relayIP, Port: int(relayPort)})
	if err != nil {
		return nil, err
	}
	defer conn.Close()

	header := []byte{0x00, 0x00, 0x00, 0x01}
	header = append(header, targetIP.To4()...)
	header = append(header, byte(targetPort>>8), byte(targetPort))

	payload := make([]byte, size)
	for i := range payload {
		payload[i] = byte(i)
	}
	packet := append(append([]byte{}, header...), payload...)

	result := &UDPTestResult{}
	readBuf := make([]byte, 64*1024)
	for i := 0; i < packets; i++ {
		conn.SetDeadline(time.Now().Add(timeout))
		if _, err = conn.Write(packet); err != nil {
			return result, err
		}
		result.Sent++
		result.BytesUp += int64(size)

		n, err := conn.Read(readBuf)
		if err != nil {
			result.LostPacket++
			continue
		}
		if n > len(header) {
			result.Received++
			result.BytesDown += int64(n - len(header))
		}
	}
	return result, nil
}
