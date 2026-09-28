package main

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"net"
	"net/http"
	"strconv"
	"time"
)

// 测试用的流量源。客户端通过代理下载或上传固定大小的数据，
// 再和统计值比对。Vision 只有内层是 TLS 时才会切换到直接拷贝，
// 所以同时提供 HTTP 和 HTTPS 两个口。

const chunkSize = 64 * 1024

// fillChunk 生成确定性的一块数据：内容只取决于块序号。
func fillChunk(buf []byte, index int) {
	seed := byte(index)
	for i := range buf {
		buf[i] = seed + byte(i)
	}
}

func sizeParam(r *http.Request) (int, error) {
	mb := r.URL.Query().Get("mb")
	if mb == "" {
		mb = "10"
	}
	n, err := strconv.Atoi(mb)
	if err != nil || n <= 0 || n > 4096 {
		return 0, fmt.Errorf("mb 参数不合法: %q", mb)
	}
	return n, nil
}

func testMux() *http.ServeMux {
	mux := http.NewServeMux()

	// 下载：返回 mb 兆确定性数据。
	// rate 参数（MB/秒）让服务端按节奏发，测「停用用户」「重建入站」这类
	// 需要传输真正在进行中的场景；不传 rate 就全速发完。
	mux.HandleFunc("/download", func(w http.ResponseWriter, r *http.Request) {
		mb, err := sizeParam(r)
		if err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		rate, _ := strconv.ParseFloat(r.URL.Query().Get("rate"), 64)
		chunks := mb * 1024 * 1024 / chunkSize
		w.Header().Set("Content-Type", "application/octet-stream")
		w.Header().Set("Content-Length", strconv.Itoa(chunks*chunkSize))
		buf := make([]byte, chunkSize)
		var perChunk time.Duration
		if rate > 0 {
			perChunk = time.Duration(float64(time.Second) * float64(chunkSize) / (rate * 1024 * 1024))
		}
		flusher, _ := w.(http.Flusher)
		for i := 0; i < chunks; i++ {
			fillChunk(buf, i)
			if _, err := w.Write(buf); err != nil {
				return
			}
			if perChunk > 0 {
				if flusher != nil {
					flusher.Flush()
				}
				time.Sleep(perChunk)
			}
		}
	})

	// 期望哈希：客户端下完自己算一遍对比
	mux.HandleFunc("/sha256", func(w http.ResponseWriter, r *http.Request) {
		mb, err := sizeParam(r)
		if err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		chunks := mb * 1024 * 1024 / chunkSize
		h := sha256.New()
		buf := make([]byte, chunkSize)
		for i := 0; i < chunks; i++ {
			fillChunk(buf, i)
			h.Write(buf)
		}
		fmt.Fprintf(w, "%s  %d\n", hex.EncodeToString(h.Sum(nil)), chunks*chunkSize)
	})

	// 上传：读完请求体，返回收到的字节数和哈希
	mux.HandleFunc("/upload", func(w http.ResponseWriter, r *http.Request) {
		h := sha256.New()
		n, err := io.Copy(h, r.Body)
		if err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		fmt.Fprintf(w, "%s  %d\n", hex.EncodeToString(h.Sum(nil)), n)
	})

	mux.HandleFunc("/healthz", func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprintln(w, "ok")
	})
	return mux
}

// StartTestServers 起 HTTP 和 HTTPS 两个流量源，都只监听 127.0.0.1。
func StartTestServers(httpAddr, httpsAddr, certPath, keyPath string) (func(), error) {
	mux := testMux()
	httpSrv := &http.Server{Addr: httpAddr, Handler: mux, ReadHeaderTimeout: 10 * time.Second}
	httpsSrv := &http.Server{Addr: httpsAddr, Handler: mux, ReadHeaderTimeout: 10 * time.Second}

	httpLn, err := net.Listen("tcp", httpAddr)
	if err != nil {
		return nil, err
	}
	httpsLn, err := net.Listen("tcp", httpsAddr)
	if err != nil {
		httpLn.Close()
		return nil, err
	}
	go httpSrv.Serve(httpLn)
	go httpsSrv.ServeTLS(httpsLn, certPath, keyPath)

	return func() {
		httpSrv.Close()
		httpsSrv.Close()
	}, nil
}
