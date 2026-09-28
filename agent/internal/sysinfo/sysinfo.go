// Package sysinfo 读服务器的网卡累计收发和 boot_id，给流量上报用。
// 设计见 main 分支 architecture.md「服务器网卡流量」。
package sysinfo

import (
	"bufio"
	"errors"
	"fmt"
	"os"
	"strconv"
	"strings"
)

// Network 是默认路由所在网卡的累计收发字节数。
type Network struct {
	// 服务器重启后会变，主控据此知道计数归零了
	BootID    string
	Interface string
	RxBytes   uint64
	TxBytes   uint64
}

// ReadNetwork 读默认路由所在网卡的累计收发和 boot_id。
func ReadNetwork() (Network, error) {
	bootID, err := readBootID()
	if err != nil {
		return Network{}, err
	}
	iface, err := defaultRouteInterface()
	if err != nil {
		return Network{}, err
	}
	rx, tx, err := interfaceCounters(iface)
	if err != nil {
		return Network{}, err
	}
	return Network{BootID: bootID, Interface: iface, RxBytes: rx, TxBytes: tx}, nil
}

func readBootID() (string, error) {
	data, err := os.ReadFile("/proc/sys/kernel/random/boot_id")
	if err != nil {
		return "", fmt.Errorf("读取 boot_id: %w", err)
	}
	return strings.TrimSpace(string(data)), nil
}

// defaultRouteInterface 从 /proc/net/route 找 IPv4 默认路由所在的网卡；有多条时取 metric 最小的。
func defaultRouteInterface() (string, error) {
	f, err := os.Open("/proc/net/route")
	if err != nil {
		return "", fmt.Errorf("读取路由表: %w", err)
	}
	defer f.Close()

	const flagUp = 0x1 // RTF_UP
	best := ""
	var bestMetric uint64
	scanner := bufio.NewScanner(f)
	scanner.Scan() // 跳过表头
	for scanner.Scan() {
		// 列：Iface Destination Gateway Flags RefCnt Use Metric Mask MTU Window IRTT
		fields := strings.Fields(scanner.Text())
		if len(fields) < 8 || fields[1] != "00000000" || fields[7] != "00000000" {
			continue
		}
		flags, err := strconv.ParseUint(fields[3], 16, 32)
		if err != nil || flags&flagUp == 0 {
			continue
		}
		metric, err := strconv.ParseUint(fields[6], 10, 64)
		if err != nil {
			continue
		}
		if best == "" || metric < bestMetric {
			best, bestMetric = fields[0], metric
		}
	}
	if err := scanner.Err(); err != nil {
		return "", fmt.Errorf("读取路由表: %w", err)
	}
	if best == "" {
		return "", errors.New("找不到默认路由所在的网卡")
	}
	return best, nil
}

// interfaceCounters 从 /proc/net/dev 读网卡的累计收发字节。
func interfaceCounters(iface string) (rx, tx uint64, err error) {
	data, err := os.ReadFile("/proc/net/dev")
	if err != nil {
		return 0, 0, fmt.Errorf("读取网卡计数: %w", err)
	}
	for _, line := range strings.Split(string(data), "\n") {
		name, rest, ok := strings.Cut(line, ":")
		if !ok || strings.TrimSpace(name) != iface {
			continue
		}
		// 接收 8 列（bytes packets errs drop fifo frame compressed multicast），然后是发送，第一列都是字节数
		fields := strings.Fields(rest)
		if len(fields) < 9 {
			return 0, 0, fmt.Errorf("/proc/net/dev 里网卡 %s 这一行格式不对", iface)
		}
		if rx, err = strconv.ParseUint(fields[0], 10, 64); err != nil {
			return 0, 0, fmt.Errorf("解析网卡 %s 的接收字节数: %w", iface, err)
		}
		if tx, err = strconv.ParseUint(fields[8], 10, 64); err != nil {
			return 0, 0, fmt.Errorf("解析网卡 %s 的发送字节数: %w", iface, err)
		}
		return rx, tx, nil
	}
	return 0, 0, fmt.Errorf("/proc/net/dev 里没有网卡 %s", iface)
}
