package state

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"

	"google.golang.org/protobuf/proto"

	agentv1 "github.com/yuuuki-creation/open-proxy/agent/internal/pb/openproxy/agent/v1"
)

// Store 把最近一次收到的期望状态保存在本地（protobuf 编码），Agent 重启时先按它启动，
// 主控连不上也能继续服务。见 protocol.md「期望状态」、architecture.md「Agent 的文件」。
type Store struct {
	path string
}

func NewStore(path string) *Store {
	return &Store{path: path}
}

// Load 读本地保存的期望状态；文件不存在时返回 nil, nil。
func (s *Store) Load() (*agentv1.DesiredState, error) {
	data, err := os.ReadFile(s.path)
	if errors.Is(err, fs.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("读取本地期望状态: %w", err)
	}
	ds := &agentv1.DesiredState{}
	if err := proto.Unmarshal(data, ds); err != nil {
		return nil, fmt.Errorf("解析本地期望状态: %w", err)
	}
	return ds, nil
}

// Save 原子写入：先写临时文件、落盘，再改名覆盖；写完立即读回校验，
// 避免写出自己读不回来的内容（妙妙屋 X 踩过的坑）。
func (s *Store) Save(ds *agentv1.DesiredState) error {
	data, err := proto.MarshalOptions{Deterministic: true}.Marshal(ds)
	if err != nil {
		return fmt.Errorf("编码期望状态: %w", err)
	}
	dir := filepath.Dir(s.path)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return fmt.Errorf("创建目录 %s: %w", dir, err)
	}
	tmp, err := os.CreateTemp(dir, ".state-*.tmp")
	if err != nil {
		return fmt.Errorf("创建临时文件: %w", err)
	}
	defer os.Remove(tmp.Name()) // 改名成功后这里什么都不做
	if _, err := tmp.Write(data); err != nil {
		tmp.Close()
		return fmt.Errorf("写临时文件: %w", err)
	}
	if err := tmp.Sync(); err != nil {
		tmp.Close()
		return fmt.Errorf("落盘: %w", err)
	}
	if err := tmp.Close(); err != nil {
		return fmt.Errorf("关闭临时文件: %w", err)
	}
	if err := os.Rename(tmp.Name(), s.path); err != nil {
		return fmt.Errorf("替换 %s: %w", s.path, err)
	}

	back, err := s.Load()
	if err != nil {
		return fmt.Errorf("读回校验: %w", err)
	}
	if !proto.Equal(back, ds) {
		return errors.New("读回校验: 读出来的内容和写进去的不一样")
	}
	return nil
}
