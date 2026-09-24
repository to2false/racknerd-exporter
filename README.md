# RackNerd Exporter

Rust 编写的 RackNerd / SolusVM Prometheus exporter，采集 VPS 状态、流量配额与用量，可选内存和磁盘统计。仅调用只读 API，每个实例监控一台 VPS。

镜像：`ghcr.io/to2false/racknerd-exporter:latest`，支持 **AMD64 / ARM64**。

## 快速启动

```sh
git clone https://github.com/to2false/racknerd-exporter.git
cd racknerd-exporter
cp .env.example .env
chmod 600 .env
```

编辑 `.env`，填入 VPS 控制台 API 页的 `RACKNERD_API_KEY`、`RACKNERD_API_HASH`，并将 `MONITORING_NETWORK` 设为现有 Prometheus 所在的 Docker 网络。

```sh
docker compose pull
docker compose up -d --no-build
```

Compose 只启动 exporter。指标地址：<http://localhost:9725/metrics>，健康检查：`/healthz`。

默认 API 缓存 **1 小时**。更多配置见 [.env.example](.env.example)，真实凭据仅存放在本地 `.env`。

## Prometheus / Grafana

在现有 Prometheus 的 `scrape_configs` 中追加任务并重载配置：

```yaml
  - job_name: racknerd
    scrape_interval: 60s
    scrape_timeout: 15s
    static_configs:
      - targets: [racknerd-exporter:9725]
```

以上地址要求两个容器处于同一网络；Prometheus 直接运行于宿主机时，使用 `127.0.0.1:9725`。

在 Grafana 导入仪表盘，选择已有的 Prometheus 数据源：

- [VPS 详情](deploy/grafana/dashboards/racknerd.json)：状态、流量配额和资源用量。
- [流量总览](deploy/grafana/dashboards/traffic-overview.json)：服务器流量、用户用量与上下行速度，需已有 sing-box exporter。

可在 Grafana 组织偏好中设为默认首页；匿名查看使用 Viewer 权限。

## 文档

[配置与指标](docs/reference.md) · [API 说明](docs/api-analysis.md) · [验证记录](docs/verification.md) · [告警示例](deploy/alerts.yml)
