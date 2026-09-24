# 配置与指标参考

## 本地运行

Rust 1.88 或更新版本；已在 Rust 1.98.1 验证。

```sh
cargo build --release --locked
export RACKNERD_API_KEY_FILE=/absolute/path/to/api-key
export RACKNERD_API_HASH_FILE=/absolute/path/to/api-hash
export RACKNERD_SERVER=my-vps
./target/release/racknerd-exporter
```

原生程序不自动加载 `.env`；Compose 会自动加载。密钥文件仅包含对应凭据，可保留末尾换行。容器使用 `_FILE` 时需自行挂载文件，并移除同名非 `_FILE` 环境变量；文件必须允许 UID 65532 读取。

| 环境变量 | 默认值 / 含义 |
|---|---|
| `RACKNERD_API_KEY` / `RACKNERD_API_KEY_FILE` | 必填，二选一 |
| `RACKNERD_API_HASH` / `RACKNERD_API_HASH_FILE` | 必填，二选一 |
| `RACKNERD_API_URL` | `https://ctrl.racknerd.com/api/client/command.php` |
| `RACKNERD_SERVER` | `racknerd`，稳定的本地 VPS 标签；不是请求参数 |
| `RACKNERD_LISTEN_ADDRESS` | 原生 `127.0.0.1:9725`；容器内部 `0.0.0.0:9725` |
| `RACKNERD_TIMEOUT_SECONDS` | `10`，范围 1–60 秒 |
| `RACKNERD_CACHE_SECONDS` | `3600`，范围 15–3600 秒 |
| `RACKNERD_COLLECT_MEMORY_DISK` | `false`；确定平台返回有效内存/磁盘统计时设 `true` |

修改 API 超时后，应将 Prometheus `scrape_timeout` 设得更长，并保证小于 `scrape_interval`。缓存于一次上游请求结束后开始计时，过期后的下一次抓取才刷新；因此默认实际请求周期可能比 1 小时稍长。成功与失败结果均缓存，多个并发抓取共享一次请求。未抓取时不调用上游 API。

## 指标语义

所有 VPS 指标都有 `server` 标签。Prometheus 自动添加 `job`、`instance`；exporter 不暴露凭据、上游错误正文、IP 或主机名。

| 指标 | 类型 | 含义 |
|---|---|---|
| `racknerd_up` | gauge | 最近一次 API 采集成功=1，失败=0 |
| `racknerd_vps_online` | gauge | 明确在线=1，离线或停用=0；未知时不导出 |
| `racknerd_vps_state_known` | gauge | API 是否返回 online/offline/disabled 状态 |
| `racknerd_vps_disabled` | gauge | 明确停用=1，在线或离线=0；未知时不导出 |
| `racknerd_bandwidth_limit_bytes` | gauge | 服务商返回的流量配额 |
| `racknerd_bandwidth_used_bytes` | gauge | 当前账期已用流量，可因重置/修正而下降 |
| `racknerd_bandwidth_remaining_bytes` | gauge | 剩余流量；超额时可能为负数 |
| `racknerd_bandwidth_used_ratio` | gauge | 已用量 / 正配额，0.8 即 80%，超额可大于 1 |
| `racknerd_resource_available{resource="bandwidth\|memory\|disk"}` | gauge | 是否返回可用数值；关闭、缺失或全零为 0 |
| `racknerd_scrape_duration_seconds` | gauge | 最近一次上游请求耗时，不是读取缓存耗时 |
| `racknerd_last_collection_timestamp_seconds` | gauge | 最近完成一次采集的 Unix 时间 |
| `racknerd_last_success_timestamp_seconds` | gauge | 最近一次成功采集时间；尚未成功为 0 |
| `racknerd_api_requests_total` / `racknerd_api_failures_total` | counter | 本进程的采集次数 / 失败次数 |
| `racknerd_exporter_build_info` | gauge | `version` 标签标识 exporter 版本 |

启用内存、磁盘后，有效数据分别导出 `racknerd_memory_*`、`racknerd_disk_*`，后缀与 bandwidth 相同。若配额为 0 且其他字段非零，保留原始字节数，不推断它是无限额，也不计算占比。

API 请求失败时 `/metrics` 仍返回 HTTP 200 和 `racknerd_up=0`，移除该次响应中的 VPS 业务指标；不会把旧用量冒充新数据。`up{job="racknerd"}` 反映 Prometheus 能否抓到 exporter，`racknerd_up` 反映 exporter 能否读到上游，二者含义不同。

CPU、负载、实时网速、应用可用性不在已确认的 API 能力内。内存/磁盘采集默认关闭，确认目标实例返回有效统计后再启用。建议通过 VPS 内的 node_exporter 补齐实时系统指标，通过应用自身指标或探测补齐业务可用性。

## 从源码构建镜像

```sh
docker build -t racknerd-exporter:local .
EXPORTER_IMAGE=racknerd-exporter:local docker compose up -d --no-build --pull never
```

`.dockerignore` 仅允许 Cargo 清单、锁文件和 `src/` 进入镜像构建上下文。`.env`、Git 元数据、文档、测试样本与本机配置不进入镜像。

## CI 与镜像发布

[GitHub Actions](https://github.com/to2false/racknerd-exporter/actions) 会运行格式检查、测试、Clippy、Gitleaks，以及两个原生架构的 Docker 构建。拉取请求只验证；`main` 和版本标签通过全部检查后发布 GHCR 多架构镜像。使用仓库的 `GITHUB_TOKEN`，无需在仓库中配置个人访问令牌或 RackNerd 凭据。Actions 固定到提交 SHA。

## 验证

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
docker build -t racknerd-exporter:local .
```

测试覆盖旧 XML 与 `<ctrl>` 响应、畸形数据、未知电源状态、超额配额、并发缓存、过期后失败与恢复、超时、HTTP 错误、禁止重定向以及凭据/错误正文不进入指标。完整验证记录见 [验证记录](verification.md)。
