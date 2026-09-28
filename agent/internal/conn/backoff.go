package conn

import (
	"math/rand/v2"
	"time"
)

// 重连退避，见 architecture.md「主控与 Agent 通信」：5 秒逐步拉长到 5 分钟，认证失败用更长的退避。
const (
	minBackoff = 5 * time.Second
	maxBackoff = 5 * time.Minute
	// Token 无效时固定等这么久：多半要管理员重新执行安装命令，重试快了也没用
	invalidTokenWait = 10 * time.Minute
	// 连接用了这么久才断，就认为之前是正常的，退避从头算
	stableDuration = time.Minute
)

// backoff 每失败一次，等待时间翻倍，最长 maxBackoff。
type backoff struct {
	current time.Duration
}

func (b *backoff) next() time.Duration {
	if b.current == 0 {
		b.current = minBackoff
	} else {
		b.current = min(b.current*2, maxBackoff)
	}
	return b.current
}

func (b *backoff) reset() {
	b.current = 0
}

// jitter 在 d 上加减最多 20% 的随机量：主控重启时，所有 Agent 不会在同一时刻一起重连。
func jitter(d time.Duration) time.Duration {
	spread := int64(d) / 5
	if spread <= 0 {
		return d
	}
	return d + time.Duration(rand.Int64N(2*spread+1)-spread)
}
