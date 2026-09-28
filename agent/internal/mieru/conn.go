package mieru

import (
	std_bufio "bufio"
	"context"
	"encoding/binary"
	"fmt"
	"io"
	"net"
	"sync"
	"time"

	apicommon "github.com/enfein/mieru/v3/apis/common"
	"github.com/enfein/mieru/v3/apis/model"
	"github.com/sagernet/sing-box/adapter"
	"github.com/sagernet/sing-box/log"
	"github.com/sagernet/sing/common/buf"
	"github.com/sagernet/sing/common/bufio"
	M "github.com/sagernet/sing/common/metadata"
	"github.com/sagernet/sing/protocol/socks"
	"github.com/sagernet/sing/protocol/socks/socks5"
)

const (
	// 读客户端 socks5 请求的时限，和 apis/server 一样
	handshakeTimeout = 10 * time.Second
	// UDP 关联建好后等第一个包的时限，和 sing-box 的 SOCKS 入站默认的 UDP 超时一样
	firstPacketTimeout = 5 * time.Minute
)

// handle 处理 mux 交来的一个连接（一个 mieru 会话）：读客户端的 socks5 请求，回应答，交给 sing-box 路由。
// mieru 客户端只发 CONNECT（TCP）和 UDP ASSOCIATE（UDP 包也在这个会话里传）；BIND 和 mita 一样回「不支持」。
func (s *Server) handle(conn net.Conn) {
	userContext, ok := conn.(apicommon.UserContext)
	if !ok {
		// mux 交出来的会话都带用户，走不到这里
		s.log.Error("mieru 会话没有用户信息，断开")
		conn.Close()
		return
	}

	// mieru 会话的读超时每读一次就清零，读一个 socks5 请求要读好几次，所以用定时器兜底：超时就关掉会话
	timer := time.AfterFunc(handshakeTimeout, func() { conn.Close() })
	request := &model.Request{}
	err := request.ReadFromSocks5(conn)
	if !timer.Stop() {
		return // 已经超时关掉了
	}
	if err != nil {
		s.log.Debug("读客户端的 socks5 请求失败，断开", "err", err)
		conn.Close()
		return
	}

	ctx := log.ContextWithNewID(s.ctx)
	metadata := adapter.InboundContext{
		Inbound:     s.tag,
		InboundType: Type,
		Source:      M.SocksaddrFromNet(conn.RemoteAddr()).Unwrap(),
		User:        userContext.UserName(),
	}
	switch request.Command {
	case socks5.CommandConnect:
		metadata.Destination = socksaddr(request.DstAddr)
		// 应答等出站连上目标以后再回（成功回绑定地址，失败回对应的错误码），和 sing-box 的 SOCKS 入站一样
		s.router.RouteConnectionEx(ctx, socks.NewLazyConn(conn, socks5.Version), metadata, nil)
	case socks5.CommandUDPAssociate:
		s.handleUDP(ctx, conn, metadata)
	default:
		if err := socks5.WriteResponse(conn, socks5.Response{ReplyCode: socks5.ReplyCodeUnsupported}); err != nil {
			s.log.Debug("回 socks5 应答失败", "err", err)
		}
		conn.Close()
	}
}

// handleUDP 处理 UDP 关联：先回成功，再等第一个包拿到目标地址，然后交给 sing-box 路由。
// 应答里的绑定地址客户端用不到（UDP 包就在这个会话里传），回 0.0.0.0:0。
func (s *Server) handleUDP(ctx context.Context, conn net.Conn, metadata adapter.InboundContext) {
	if err := socks5.WriteResponse(conn, socks5.Response{ReplyCode: socks5.ReplyCodeSuccess}); err != nil {
		s.log.Debug("回 socks5 应答失败", "err", err)
		conn.Close()
		return
	}
	packetConn := newPacketConn(conn)
	timer := time.AfterFunc(firstPacketTimeout, func() { conn.Close() })
	buffer := buf.NewPacket()
	destination, err := packetConn.ReadPacket(buffer)
	if !timer.Stop() {
		buffer.Release()
		return
	}
	if err != nil {
		buffer.Release()
		s.log.Debug("读 UDP 关联的第一个包失败，断开", "err", err)
		conn.Close()
		return
	}
	// 路由按第一个包的目标地址选规则（和 sing-box 的 SOCKS 入站一样），这个包缓存起来照常发出去
	metadata.Destination = destination
	s.router.RoutePacketConnectionEx(ctx, bufio.NewCachedPacketConn(packetConn, buffer, destination), metadata, nil)
}

// socksaddr 把 mieru 请求里的目标地址转成 sing 的地址。
func socksaddr(addr model.AddrSpec) M.Socksaddr {
	port := uint16(addr.Port)
	if addr.FQDN != "" {
		return M.ParseSocksaddrHostPort(addr.FQDN, port)
	}
	return M.SocksaddrFrom(M.AddrFromIP(addr.IP), port).Unwrap()
}

// UDP 包在会话里的格式和 mieru 的 PacketOverStreamTunnel 一样：前缀 0x00、2 字节长度（大端）、内容、后缀 0xff。
// 内容是 socks5 的 UDP 头（2 字节保留、1 字节分片号、地址）加数据，mieru 客户端和 mita 都这样用。
const (
	packetPrefix = 0x00
	packetSuffix = 0xff
	// socks5 UDP 头里地址前面的 3 个字节：保留、保留、分片号
	udpHeaderReserved = 3
)

// packetConn 把一个 mieru 会话里的 UDP 关联包装成 sing 的 N.PacketConn。
// sing-box 的连接管理用一个 goroutine 读、一个 goroutine 写。
type packetConn struct {
	conn   net.Conn
	reader *std_bufio.Reader

	writeMu sync.Mutex
}

func newPacketConn(conn net.Conn) *packetConn {
	return &packetConn{conn: conn, reader: std_bufio.NewReader(conn)}
}

// ReadPacket 读下一个包，数据放进 buffer，返回它的目标地址。
// 格式不对时返回错误，整个 UDP 关联随之关闭（和 mita 一样）；比缓冲区还大的包丢掉，接着读下一个。
func (c *packetConn) ReadPacket(buffer *buf.Buffer) (M.Socksaddr, error) {
	for {
		var head [3]byte
		if _, err := io.ReadFull(c.reader, head[:]); err != nil {
			return M.Socksaddr{}, err
		}
		if head[0] != packetPrefix {
			return M.Socksaddr{}, fmt.Errorf("收到的 UDP 包前缀是 0x%02x，应该是 0x00", head[0])
		}
		length := int(binary.BigEndian.Uint16(head[1:]))
		if length > buffer.FreeLen() {
			// 超过缓冲区的包（正常的 UDP 包到不了这么大）：连同后缀一起跳过
			if _, err := c.reader.Discard(length + 1); err != nil {
				return M.Socksaddr{}, err
			}
			continue
		}
		if _, err := buffer.ReadFullFrom(c.reader, length); err != nil {
			return M.Socksaddr{}, err
		}
		suffix, err := c.reader.ReadByte()
		if err != nil {
			return M.Socksaddr{}, err
		}
		if suffix != packetSuffix {
			return M.Socksaddr{}, fmt.Errorf("收到的 UDP 包后缀是 0x%02x，应该是 0xff", suffix)
		}
		if buffer.Len() < udpHeaderReserved {
			return M.Socksaddr{}, fmt.Errorf("收到的 UDP 包只有 %d 字节，放不下 socks5 UDP 头", buffer.Len())
		}
		if buffer.Byte(2) != 0 {
			return M.Socksaddr{}, fmt.Errorf("收到分片的 UDP 包（分片号 %d），不支持", buffer.Byte(2))
		}
		buffer.Advance(udpHeaderReserved)
		destination, err := M.SocksaddrSerializer.ReadAddrPort(buffer)
		if err != nil {
			return M.Socksaddr{}, fmt.Errorf("解析 UDP 包的目标地址: %w", err)
		}
		return destination.Unwrap(), nil
	}
}

// WritePacket 把一个包发回客户端，source 是这个包的来源地址。按约定由这里释放 buffer。
func (c *packetConn) WritePacket(buffer *buf.Buffer, source M.Socksaddr) error {
	defer buffer.Release()
	size := udpHeaderReserved + M.SocksaddrSerializer.AddrPortLen(source) + buffer.Len()
	if size > 0xffff {
		return fmt.Errorf("要发的 UDP 包有 %d 字节，超过了 65535", size)
	}
	packet := buf.NewSize(3 + size + 1)
	defer packet.Release()
	_ = packet.WriteByte(packetPrefix)
	binary.BigEndian.PutUint16(packet.Extend(2), uint16(size))
	_ = packet.WriteZeroN(udpHeaderReserved)
	if err := M.SocksaddrSerializer.WriteAddrPort(packet, source); err != nil {
		return fmt.Errorf("写 UDP 包的来源地址: %w", err)
	}
	_, _ = packet.Write(buffer.Bytes())
	_ = packet.WriteByte(packetSuffix)

	c.writeMu.Lock()
	defer c.writeMu.Unlock()
	_, err := c.conn.Write(packet.Bytes())
	return err
}

func (c *packetConn) Close() error                       { return c.conn.Close() }
func (c *packetConn) LocalAddr() net.Addr                { return c.conn.LocalAddr() }
func (c *packetConn) SetDeadline(t time.Time) error      { return c.conn.SetDeadline(t) }
func (c *packetConn) SetReadDeadline(t time.Time) error  { return c.conn.SetReadDeadline(t) }
func (c *packetConn) SetWriteDeadline(t time.Time) error { return c.conn.SetWriteDeadline(t) }
