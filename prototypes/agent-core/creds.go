package main

import (
	"crypto/ecdh"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/base64"
	"encoding/hex"
	"encoding/pem"
	"fmt"
	"math/big"
	"net"
	"os"
	"path/filepath"
	"time"

	"github.com/gofrs/uuid/v5"
)

// User 是原型里的一个用户。正式实现里每个用户一套凭据、所有节点通用，
// 这里保持同样的形状：一个 UUID 加一个密码，各协议按需取用。
type User struct {
	Name     string `json:"name"`
	UUID     string `json:"uuid"`     // VLESS
	Password string `json:"password"` // Hysteria2、AnyTLS
	SSKey    string `json:"ss_key"`   // Shadowsocks 2022 的用户密钥（base64）
}

// Creds 是本次运行用到的全部凭据和密钥。
type Creds struct {
	Users []User `json:"users"`

	RealityPrivateKey string `json:"reality_private_key"`
	RealityPublicKey  string `json:"reality_public_key"`
	RealityShortID    string `json:"reality_short_id"`

	SSServerKey string `json:"ss_server_key"` // Shadowsocks 2022 的服务端主密钥

	CertPath string `json:"cert_path"`
	KeyPath  string `json:"key_path"`
}

func randomBase64(n int) (string, error) {
	b := make([]byte, n)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return base64.StdEncoding.EncodeToString(b), nil
}

// generateRealityKeyPair 生成 REALITY 用的 X25519 密钥对，
// 编码方式和 sing-box、Xray 一致：base64 raw url。
func generateRealityKeyPair() (privateKey string, publicKey string, err error) {
	key, err := ecdh.X25519().GenerateKey(rand.Reader)
	if err != nil {
		return "", "", err
	}
	enc := base64.RawURLEncoding
	return enc.EncodeToString(key.Bytes()), enc.EncodeToString(key.PublicKey().Bytes()), nil
}

// NewUser 生成一个用户的全套凭据。
func NewUser(name string, ssKeyLen int) (*User, error) {
	id, err := uuid.NewV4()
	if err != nil {
		return nil, err
	}
	password, err := randomBase64(16)
	if err != nil {
		return nil, err
	}
	ssKey, err := randomBase64(ssKeyLen)
	if err != nil {
		return nil, err
	}
	return &User{Name: name, UUID: id.String(), Password: password, SSKey: ssKey}, nil
}

// NewCreds 生成本次运行的凭据，并把自签证书写到 outDir。
// ssKeyLen 取决于加密方式：aes-128-gcm 是 16 字节，aes-256-gcm 是 32 字节。
func NewCreds(userCount int, ssKeyLen int, outDir string, certHosts []string) (*Creds, error) {
	c := &Creds{}
	for i := 1; i <= userCount; i++ {
		u, err := NewUser(fmt.Sprintf("u%d", i), ssKeyLen)
		if err != nil {
			return nil, err
		}
		c.Users = append(c.Users, *u)
	}

	var err error
	if c.RealityPrivateKey, c.RealityPublicKey, err = generateRealityKeyPair(); err != nil {
		return nil, err
	}
	shortID := make([]byte, 4)
	if _, err = rand.Read(shortID); err != nil {
		return nil, err
	}
	c.RealityShortID = hex.EncodeToString(shortID)

	if c.SSServerKey, err = randomBase64(ssKeyLen); err != nil {
		return nil, err
	}

	if c.CertPath, c.KeyPath, err = writeSelfSignedCert(outDir, certHosts); err != nil {
		return nil, err
	}
	return c, nil
}

// writeSelfSignedCert 生成自签证书，给 Hysteria2 和 AnyTLS 用。
// 原型里客户端跳过校验；正式实现是上报证书指纹、订阅里固定指纹。
func writeSelfSignedCert(outDir string, hosts []string) (certPath string, keyPath string, err error) {
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return "", "", err
	}
	serial, err := rand.Int(rand.Reader, new(big.Int).Lsh(big.NewInt(1), 128))
	if err != nil {
		return "", "", err
	}
	tmpl := x509.Certificate{
		SerialNumber:          serial,
		Subject:               pkix.Name{CommonName: hosts[0]},
		NotBefore:             time.Now().Add(-time.Hour),
		NotAfter:              time.Now().Add(365 * 24 * time.Hour),
		KeyUsage:              x509.KeyUsageDigitalSignature | x509.KeyUsageCertSign,
		ExtKeyUsage:           []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
		BasicConstraintsValid: true,
	}
	for _, h := range hosts {
		if ip := net.ParseIP(h); ip != nil {
			tmpl.IPAddresses = append(tmpl.IPAddresses, ip)
		} else {
			tmpl.DNSNames = append(tmpl.DNSNames, h)
		}
	}
	der, err := x509.CreateCertificate(rand.Reader, &tmpl, &tmpl, &key.PublicKey, key)
	if err != nil {
		return "", "", err
	}
	keyDER, err := x509.MarshalECPrivateKey(key)
	if err != nil {
		return "", "", err
	}

	certPath = filepath.Join(outDir, "server.crt")
	keyPath = filepath.Join(outDir, "server.key")
	if err = os.WriteFile(certPath, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: der}), 0o644); err != nil {
		return "", "", err
	}
	if err = os.WriteFile(keyPath, pem.EncodeToMemory(&pem.Block{Type: "EC PRIVATE KEY", Bytes: keyDER}), 0o600); err != nil {
		return "", "", err
	}
	return certPath, keyPath, nil
}
