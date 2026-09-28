-- 全部表。字段含义和规则见 main 分支 docs/design-docs/database.md。
-- 约定：主键自增（AUTOINCREMENT，删掉的 ID 不再复用，历史流量不会串到新对象上）；
-- 时间是 Unix 毫秒（UTC）；日期是管理员时区的 YYYY-MM-DD 文本；流量是字节。

-- 唯一的管理员
CREATE TABLE admin (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    username      TEXT    NOT NULL,
    password_hash TEXT    NOT NULL, -- Argon2id
    updated_at    INTEGER NOT NULL
);

-- 登录会话：Cookie 里是 Token 明文，这里只存 SHA-256
CREATE TABLE sessions (
    token_hash   TEXT    PRIMARY KEY,
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    ip           TEXT    NOT NULL DEFAULT '',
    user_agent   TEXT    NOT NULL DEFAULT ''
);

-- 全局设置，值是 JSON
CREATE TABLE settings (
    key        TEXT    PRIMARY KEY,
    value      TEXT    NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE servers (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    name                TEXT    NOT NULL UNIQUE,
    address             TEXT    NOT NULL,
    port_range_start    INTEGER NOT NULL DEFAULT 10000,
    port_range_end      INTEGER NOT NULL DEFAULT 60000,
    cert_mode           TEXT    NOT NULL DEFAULT 'self_signed' CHECK (cert_mode IN ('acme', 'self_signed')),
    cert_domain         TEXT,
    traffic_quota_bytes INTEGER,
    traffic_reset_day   INTEGER CHECK (traffic_reset_day BETWEEN 1 AND 31),
    token_hash          TEXT    NOT NULL UNIQUE,
    -- 期望状态：当前版本和内容哈希
    state_version       INTEGER NOT NULL DEFAULT 0,
    state_hash          TEXT    NOT NULL DEFAULT '',
    -- Agent 最近一次 StateReport
    applied_version     INTEGER NOT NULL DEFAULT 0,
    apply_failures      TEXT    NOT NULL DEFAULT '[]',
    -- Agent 最近一次 Hello；实例 ID 是 u64，按位存成 i64
    agent_version       TEXT    NOT NULL DEFAULT '',
    agent_arch          TEXT    NOT NULL DEFAULT '',
    agent_instance_id   INTEGER NOT NULL DEFAULT 0,
    rolled_back_from    TEXT    NOT NULL DEFAULT '',
    last_seen_at        INTEGER,
    -- 网卡计数上一次的原始值
    nic_boot_id         TEXT    NOT NULL DEFAULT '',
    nic_interface       TEXT    NOT NULL DEFAULT '',
    nic_last_rx         INTEGER NOT NULL DEFAULT 0,
    nic_last_tx         INTEGER NOT NULL DEFAULT 0,
    created_at          INTEGER NOT NULL
);

-- 已删除服务器的 Token 哈希：它的 Agent 下次连上来时通知它卸载
CREATE TABLE deleted_servers (
    token_hash TEXT    PRIMARY KEY,
    name       TEXT    NOT NULL,
    deleted_at INTEGER NOT NULL
);

-- 证书：server_id 为空的是主控自己的 HTTPS 证书，只能有一张；每台服务器最多一张
CREATE TABLE certificates (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    server_id  INTEGER REFERENCES servers (id) ON DELETE CASCADE,
    kind       TEXT    NOT NULL CHECK (kind IN ('acme', 'self_signed')),
    domain     TEXT,
    cert_pem   TEXT    NOT NULL,
    key_pem    TEXT    NOT NULL,
    sha256     TEXT    NOT NULL,
    not_after  INTEGER NOT NULL,
    renewed_at INTEGER NOT NULL,
    last_error TEXT    NOT NULL DEFAULT ''
);
CREATE UNIQUE INDEX certificates_owner ON certificates (IFNULL(server_id, 0));

-- 落地出口：自建的由 landing_server_id 那台服务器提供 SOCKS5 入站
CREATE TABLE exits (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    name              TEXT    NOT NULL UNIQUE,
    kind              TEXT    NOT NULL CHECK (kind IN ('third_party', 'self_built')),
    host              TEXT    NOT NULL DEFAULT '',
    port              INTEGER NOT NULL,
    username          TEXT    NOT NULL DEFAULT '',
    password          TEXT    NOT NULL DEFAULT '',
    -- 不级联删除：落地机要先删掉这个出口才能删
    landing_server_id INTEGER UNIQUE REFERENCES servers (id),
    created_at        INTEGER NOT NULL
);

CREATE TABLE nodes (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    server_id      INTEGER NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    name           TEXT    NOT NULL,
    protocol       TEXT    NOT NULL CHECK (protocol IN ('vless_reality', 'hysteria2', 'anytls', 'shadowsocks2022', 'mieru')),
    port           INTEGER NOT NULL,
    hop_port_start INTEGER,
    hop_port_end   INTEGER,
    params         TEXT    NOT NULL DEFAULT '{}', -- JSON，各协议自己的参数
    address        TEXT,                          -- 为空时用服务器的地址
    exit_id        INTEGER REFERENCES exits (id), -- 为空表示直连；有节点在用的出口不能删
    enabled        INTEGER NOT NULL DEFAULT 1,
    sort_order     INTEGER NOT NULL DEFAULT 0,
    created_at     INTEGER NOT NULL,
    UNIQUE (server_id, port)
);

CREATE TABLE plans (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    name                TEXT    NOT NULL UNIQUE,
    traffic_quota_bytes INTEGER, -- 为空表示不限
    created_at          INTEGER NOT NULL
);

CREATE TABLE plan_nodes (
    plan_id INTEGER NOT NULL REFERENCES plans (id) ON DELETE CASCADE,
    node_id INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    PRIMARY KEY (plan_id, node_id)
);

CREATE TABLE users (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    name           TEXT    NOT NULL UNIQUE,
    remark         TEXT    NOT NULL DEFAULT '',
    plan_id        INTEGER NOT NULL REFERENCES plans (id), -- 有用户绑定的套餐不能删
    enabled        INTEGER NOT NULL DEFAULT 1,             -- 管理员手动停用时为 0
    started_on     TEXT    NOT NULL,                       -- 开通日，决定每月哪天重置
    expires_on     TEXT,                                   -- 到期日，为空表示永久
    uuid           TEXT    NOT NULL UNIQUE,
    password       TEXT    NOT NULL,
    ss_key         TEXT    NOT NULL,
    sub_token      TEXT    NOT NULL UNIQUE,
    up_total       INTEGER NOT NULL DEFAULT 0,
    down_total     INTEGER NOT NULL DEFAULT 0,
    period_base    INTEGER NOT NULL DEFAULT 0,             -- 本周期起点时的 up_total + down_total
    last_reset_at  INTEGER,
    blocked_reason TEXT    NOT NULL DEFAULT '' CHECK (blocked_reason IN ('', 'manual', 'expired', 'over_quota')),
    blocked_since  INTEGER,
    created_at     INTEGER NOT NULL
);

-- Agent 上报的累计值上一次是多少；Agent 换了实例 ID 时删掉这台服务器的全部行
CREATE TABLE traffic_counters (
    server_id INTEGER NOT NULL,
    node_id   INTEGER NOT NULL,
    user_id   INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    last_up   INTEGER NOT NULL,
    last_down INTEGER NOT NULL,
    PRIMARY KEY (server_id, node_id, user_id)
);

-- 用户日账本。node_id、server_id 不设外键：节点、服务器删掉后历史流量还在
CREATE TABLE traffic_daily (
    day       TEXT    NOT NULL,
    user_id   INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    node_id   INTEGER NOT NULL,
    server_id INTEGER NOT NULL,
    up        INTEGER NOT NULL DEFAULT 0,
    down      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (day, user_id, node_id)
);
CREATE INDEX traffic_daily_user ON traffic_daily (user_id, day);
CREATE INDEX traffic_daily_node ON traffic_daily (node_id, day);
CREATE INDEX traffic_daily_server ON traffic_daily (server_id, day);

-- 服务器日账本：网卡收发
CREATE TABLE server_traffic_daily (
    day       TEXT    NOT NULL,
    server_id INTEGER NOT NULL,
    rx        INTEGER NOT NULL DEFAULT 0,
    tx        INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (day, server_id)
);
