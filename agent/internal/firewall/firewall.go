// Package firewall 管理 Hysteria2 端口跳跃的 nftables 规则：一张 inet 族的表（默认 op_agent），
// 里面一条 nat 链，每个开了端口跳跃的节点一条规则，把发往本机的一段 UDP 端口转到节点的实际端口。
// 每次都整体重建（删表再建，一次原子提交），所以启动时、每次应用期望状态时都可以放心重做，
// 被别人改过的规则也会被纠正。设计见 main 分支 nodes.md「Hysteria2 端口跳跃与混淆」。
// 用 github.com/google/nftables 直接走 netlink，不依赖系统里有没有 nft 命令。
package firewall

import (
	"fmt"
	"strconv"

	"github.com/google/nftables"
	"github.com/google/nftables/binaryutil"
	"github.com/google/nftables/expr"
	"github.com/google/nftables/userdata"
	"golang.org/x/sys/unix"
)

// DefaultTable 是 Agent 用的表名（architecture.md「Agent 的文件」）。
const DefaultTable = "op_agent"

const chainName = "port_hopping"

// Rule 是一个节点的端口跳跃：发往本机的 IPv4 UDP 端口 [Start, End] 转到 Port。
type Rule struct {
	NodeID uint64
	Start  uint16
	End    uint16
	Port   uint16
}

// Firewall 管一张 nftables 表。
type Firewall struct {
	table string
}

func New(table string) *Firewall {
	return &Firewall{table: table}
}

// Table 返回表名。
func (f *Firewall) Table() string {
	return f.table
}

// Apply 按 rules 整体重建表：删掉原来的表，再建表、链和规则，在一次提交里完成，
// 要么全部生效，要么什么都不变。rules 为空时只删表。
func (f *Firewall) Apply(rules []Rule) error {
	conn, err := nftables.New()
	if err != nil {
		return fmt.Errorf("连接 nftables: %w", err)
	}
	table := &nftables.Table{Family: nftables.TableFamilyINet, Name: f.table}
	// 先加再删：表不存在时删除也不会失败，和后面的新建在同一次提交里
	conn.AddTable(table)
	conn.DelTable(table)
	if len(rules) > 0 {
		table = conn.AddTable(&nftables.Table{Family: nftables.TableFamilyINet, Name: f.table})
		chain := conn.AddChain(&nftables.Chain{
			Name:     chainName,
			Table:    table,
			Type:     nftables.ChainTypeNAT,
			Hooknum:  nftables.ChainHookPrerouting,
			Priority: nftables.ChainPriorityNATDest,
		})
		for _, r := range rules {
			conn.AddRule(&nftables.Rule{
				Table:    table,
				Chain:    chain,
				Exprs:    hoppingExprs(r),
				UserData: userdata.AppendString(nil, userdata.TypeComment, "node-"+strconv.FormatUint(r.NodeID, 10)),
			})
		}
	}
	if err := conn.Flush(); err != nil {
		return fmt.Errorf("提交 nftables 表 %s: %w", f.table, err)
	}
	return nil
}

// Remove 删掉整张表（卸载时用），表不存在时不报错。
func (f *Firewall) Remove() error {
	return f.Apply(nil)
}

// hoppingExprs 生成一条规则，相当于：
//
//	meta nfproto ipv4 fib daddr type local udp dport Start-End redirect to :Port comment "node-<ID>"
//
// 只转发往本机的包（fib daddr type local）：经过本机转发的包（例如容器的流量）不动。只管 IPv4：节点只监听 IPv4。
func hoppingExprs(r Rule) []expr.Any {
	return []expr.Any{
		&expr.Meta{Key: expr.MetaKeyNFPROTO, Register: 1},
		&expr.Cmp{Op: expr.CmpOpEq, Register: 1, Data: []byte{unix.NFPROTO_IPV4}},
		&expr.Fib{Register: 1, FlagDADDR: true, ResultADDRTYPE: true},
		&expr.Cmp{Op: expr.CmpOpEq, Register: 1, Data: binaryutil.NativeEndian.PutUint32(unix.RTN_LOCAL)},
		&expr.Meta{Key: expr.MetaKeyL4PROTO, Register: 1},
		&expr.Cmp{Op: expr.CmpOpEq, Register: 1, Data: []byte{unix.IPPROTO_UDP}},
		&expr.Payload{DestRegister: 1, Base: expr.PayloadBaseTransportHeader, Offset: 2, Len: 2},
		&expr.Range{Op: expr.CmpOpEq, Register: 1, FromData: binaryutil.BigEndian.PutUint16(r.Start), ToData: binaryutil.BigEndian.PutUint16(r.End)},
		&expr.Immediate{Register: 1, Data: binaryutil.BigEndian.PutUint16(r.Port)},
		&expr.Redir{RegisterProtoMin: 1},
	}
}
