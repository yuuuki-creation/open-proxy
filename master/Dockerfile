# op-master 的 Docker 镜像。不在这里编译：发布工作流（.github/workflows/release.yml）已经编译好
# 静态链接的单文件（内置 Agent 和面板），放在构建上下文的 dist/ 下。
# 用 root 运行：要监听 443，数据卷也归 root。
FROM gcr.io/distroless/static-debian12
ARG TARGETARCH
COPY --chmod=755 dist/op-master-linux-${TARGETARCH} /usr/local/bin/op-master
ENV OP_MASTER_DATA_DIR=/var/lib/op-master
VOLUME /var/lib/op-master
EXPOSE 443
ENTRYPOINT ["/usr/local/bin/op-master"]
