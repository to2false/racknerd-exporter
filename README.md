# RackNerd Exporter

Rust 编写的 RackNerd / SolusVM 兼容客户端 API exporter。通过 Prometheus 保存指标，再用 Grafana 查看 VPS 电源状态、服务商统计的流量配额、已用量与剩余量。

```text
RackNerd client API ← HTTPS POST ← Rust exporter:9725
                                       ↑ /metrics
                                   Prometheus ← Grafana
```

本项目为独立 Cargo 项目。只调用 `action=info`，不提供开机、关机、重启接口。一个进程监控一台 VPS；多台 VPS 启动多个实例，每台使用自己的 Key/Hash 和 `RACKNERD_SERVER` 标签。

## 接口能力

默认端点为 `https://ctrl.racknerd.com/api/client/command.php`。使用 HTTPS POST `action=info` 查询，解析带 `<ctrl>` 根节点的 XML，也兼容旧 SolusVM XML。已验证电源状态、流量配额及可选的内存/磁盘字段；后两项是否可用取决于服务商和虚拟化类型。详见 [API 分析](docs/api-analysis.md)。

## 预构建镜像

镜像：`ghcr.io/to2false/racknerd-exporter`，支持 `linux/amd64` 与 `linux/arm64`。

```sh
docker pull ghcr.io/to2false/racknerd-exporter:latest
```

`main` 分支通过验证后发布 `latest` 和 `sha-<完整提交 SHA>`；推送 `vX.Y.Z` 标签会发布同名版本镜像。生产部署可固定提交标签或镜像 digest。镜像以 UID/GID 65532 运行，凭据仅在运行时传入。

## Docker Compose 启动

```sh
cp -n .env.example .env
chmod 600 .env
# 在编辑器中填写 RACKNERD_API_KEY、RACKNERD_API_HASH
# MONITORING_NETWORK 填现有 Prometheus 所在的 Docker 网络，默认 lan
docker compose pull
docker compose up -d --no-build
```

从 VPS 的 API 页取得已经启用的 Key 和 Hash，直接填入；不要自行重新计算 Hash。若控制台显示另一个接口地址，请修改 `RACKNERD_API_URL`。`.env` 中包含 `$`、`#` 等特殊字符的值可用单引号包裹。真实凭据不要提交到版本库。

Compose 只启动 exporter，复用已有的外部 Docker 网络；不会创建 Prometheus、Grafana 或监控数据卷。默认 exporter 地址为 <http://localhost:9725/metrics>，进程健康检查为 `/healthz`。

端口有冲突时修改 `.env` 的 `EXPORTER_PORT`。宿主机端口绑定 `127.0.0.1`；Prometheus 通过共享网络中的 `racknerd-exporter:9725` 抓取。`docker compose down` 只停止本项目的 exporter。

## 接入已有 Prometheus / Grafana

在现有 Prometheus 的 `scrape_configs` 下追加任务，保留已有任务：

```yaml
scrape_configs:
  - job_name: racknerd
    scrape_interval: 60s
    scrape_timeout: 15s
    static_configs:
      - targets: [racknerd-exporter:9725]
```

此地址适用于 Prometheus 与 exporter 共享 Docker 网络。Prometheus 直接运行于同一宿主机时，改用 `127.0.0.1:9725`。验证配置后重载 Prometheus，例如 `docker exec prometheus promtool check config /etc/prometheus/prometheus.yml`，再执行 `docker kill --signal=HUP prometheus`。

在 Grafana 导入 [deploy/grafana/dashboards/racknerd.json](deploy/grafana/dashboards/racknerd.json)，将顶部 **Prometheus** 变量选成现有数据源。多台 VPS 会显示在 **VPS** 下拉菜单中。

RackNerd 详情仪表盘 UID 为 `racknerd-overview`，地址路径为 `/d/racknerd-overview`。

### 默认流量汇总仪表盘

项目还提供 [服务器与用户流量汇总仪表盘](deploy/grafana/dashboards/traffic-overview.json)，UID 为 `traffic-overview`，可与已有 sing-box exporter 指标配合使用。

- 服务器：账期已用量、剩余额度、配额使用率和数据更新时间。RackNerd API 缓存保持 1 小时。
- sing-box：所选时段总用量、在线用户、用户流量排行、上下行速度及连接数。默认查看近 24 小时，每 15 秒刷新。
- 用量使用 `increase(singbox_traffic_bytes_total[$__range])`，速度使用近 1 分钟的 `rate`，单位为 B/s（字节/秒）；计数器重置由 Prometheus 处理，采集中断仍可能漏计。
- 空用户标签显示为“未归属”。RackNerd 服务器与 sing-box 实例独立筛选，不视为同一台机器，也不将代理流量当作服务商账单。

需要匿名查看时，可在自己的 Grafana 中为目标组织启用匿名 Viewer 权限，并保留适当的网络访问控制。本项目不会自动修改 Grafana 权限。组织默认首页可设置为 `homeDashboardUID=traffic-overview`；个人首页设置会覆盖组织默认值。

在 Grafana 中导入 JSON 后，选择已有 Prometheus 数据源，并在组织偏好中将它设为首页。仪表盘依赖名为 `racknerd` 和 `singbox` 的抓取任务，默认 Compose 仍只启动 RackNerd exporter。

[deploy/alerts.yml](deploy/alerts.yml) 提供 exporter 不可达、API 失败、VPS 离线、流量超过 80%、电源状态未知的示例告警规则。如需启用规则，将文件挂载到现有 Prometheus 并添加到 `rule_files`。通知还需配置 Alertmanager 或 Grafana 联系点。`deploy/prometheus.yml` 和 Grafana provisioning 文件仅供参考，默认 Compose 不加载它们。

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

测试覆盖旧 XML 与 `<ctrl>` 响应、畸形数据、未知电源状态、超额配额、并发缓存、过期后失败与恢复、超时、HTTP 错误、禁止重定向以及凭据/错误正文不进入指标。完整验证记录见 [docs/verification.md](docs/verification.md)。
