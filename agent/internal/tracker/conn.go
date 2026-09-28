package tracker

import (
	"net"
	"sync"
	"time"

	"github.com/sagernet/sing/common/buf"
	M "github.com/sagernet/sing/common/metadata"
	N "github.com/sagernet/sing/common/network"
)

// trackedConn 包在计数连接外面，只为在连接关闭时注销登记。
// 实现 sing 的「可解包」接口，让上层的拷贝该怎么优化还怎么优化；计数由内层的计数连接负责。
type trackedConn struct {
	net.Conn
	tracker *Tracker
	key     Key
	id      uint64
	once    sync.Once
}

func (c *trackedConn) Close() error {
	c.once.Do(func() { c.tracker.deregister(c.key, c.id) })
	return c.Conn.Close()
}

func (c *trackedConn) Upstream() any           { return c.Conn }
func (c *trackedConn) UpstreamReader() any     { return c.Conn }
func (c *trackedConn) UpstreamWriter() any     { return c.Conn }
func (c *trackedConn) ReaderReplaceable() bool { return true }
func (c *trackedConn) WriterReplaceable() bool { return true }

// trackedPacketConn 是 UDP 版本的 trackedConn。
type trackedPacketConn struct {
	N.PacketConn
	tracker *Tracker
	key     Key
	id      uint64
	once    sync.Once
}

func (c *trackedPacketConn) Close() error {
	c.once.Do(func() { c.tracker.deregister(c.key, c.id) })
	return c.PacketConn.Close()
}

func (c *trackedPacketConn) Upstream() any { return c.PacketConn }

func (c *trackedPacketConn) ReadPacket(buffer *buf.Buffer) (M.Socksaddr, error) {
	return c.PacketConn.ReadPacket(buffer)
}

func (c *trackedPacketConn) WritePacket(buffer *buf.Buffer, destination M.Socksaddr) error {
	return c.PacketConn.WritePacket(buffer, destination)
}

func (c *trackedPacketConn) SetDeadline(t time.Time) error { return c.PacketConn.SetDeadline(t) }

func (c *trackedPacketConn) SetReadDeadline(t time.Time) error {
	return c.PacketConn.SetReadDeadline(t)
}

func (c *trackedPacketConn) SetWriteDeadline(t time.Time) error {
	return c.PacketConn.SetWriteDeadline(t)
}
