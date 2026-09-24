# RackNerd API 接入分析

## 协议与范围

默认端点为 `https://ctrl.racknerd.com/api/client/command.php`。VPS 控制台的 API 页提供 Key 和 Hash，凭据限定目标 VPS；管理页的 `vserverid` 不是 API 请求参数。

RackNerd 控制台说明支持 GET / POST，默认 XML，`rdtype=json` 可选择 JSON。Exporter 使用 HTTPS POST 表单和已验证的 XML 格式，避免将凭据写入 URL。响应带 `<ctrl>` 根节点；也兼容旧 SolusVM 的相邻 XML 元素。

| 参数 | 用途 |
|---|---|
| `key`、`hash` | API 页提供的有效凭据，Hash 无需再次计算 |
| `action=info` | 固定的只读查询动作 |
| `status=true` | 返回 `vmstat` |
| `bw=true` | 返回流量配额、用量与余量 |
| `hdd=true`、`mem=true` | 启用可选磁盘和内存采集后发送 |
| `ipaddr=true` | API 支持，exporter 不请求 |

API 还提供开机、关机、重启等动作，exporter 不暴露这些入口。HTTP 200 不代表业务成功：必须同时满足 XML 的 `status=success`。鉴权错误、HTML 页面、畸形 XML、无效数字和超大响应均视为采集失败。

## 响应示例

以下数据完全模拟，不含实际 VPS 的身份或用量。资源字段格式为 `total_bytes,used_bytes,free_bytes,percent_used`。

```xml
<ctrl>
  <status>success</status>
  <vmstat>online</vmstat>
  <bw>1073741824000,268435456000,805306368000,25</bw>
  <hdd>21474836480,5368709120,16106127360,25</hdd>
  <mem>1073741824,268435456,805306368,25</mem>
</ctrl>
```

示例流量配额 1000 GiB、已用 250 GiB；磁盘配额 20 GiB、已用 5 GiB；内存配额 1 GiB、已用 256 MiB。上游百分比可能为整数舍入值，exporter 用字节数计算比例。

已验证部分 KVM 实例可返回非零磁盘和内存统计，但不同服务商或实例可能不提供。因此默认关闭这两项采集；确认目标实例有效后再启用。服务商统计不等于虚拟机内部实时数据。

## 状态与时间语义

- 已确认状态包含 `online`、`offline`、`disabled`；分别导出在线和停用指标。其他值保持未知。
- 调查时控制台注明流量每 5 分钟更新，每月 1 日 00:00 UTC 重置；实际账期以服务商为准。
- API 默认缓存 3600 秒，成功与失败结果都缓存。过期后的下一次 Prometheus 抓取才请求上游；无人抓取时不请求 API。
- 账期用量可能下降或被修正，因此导出 gauge，不能直接用 `rate()` 当作实时网卡速率。
- 全零或空的资源字段视为不可用；配额为 0 时不推断无限额，也不计算比例；允许超额和负剩余量。
- CPU、负载、实时收发速率、应用可用性不在本接口已确认的能力内。

## 凭据与错误处理

若控制台显示凭据可用，但 API 返回鉴权失败，应核对 API 地址和 Key/Hash，并由账户持有人在控制台处理凭据。不要将凭据或原始响应贴入公开 issue。

本地 `.env` 被 Git 忽略；Docker 构建上下文仅包含清单、锁文件和源码。运行时保持 TLS 证书与主机名校验，不跟随重定向，不记录上游原始正文、凭据或带鉴权参数的 URL。

## Grafana 接入

Grafana 读取 Prometheus，API 凭据仅由 exporter 使用。项目提供 RackNerd 详情、服务器与 sing-box 用户流量汇总仪表盘，以及示例告警规则；默认 Compose 只启动 exporter。

公开参考：[SolusVM 客户端 API](https://docs.solusvm.com/en/solusvm1/api/client/client-overview/)、[Prometheus exporter 指南](https://prometheus.io/docs/instrumenting/writing_exporters/)、[Grafana provisioning](https://grafana.com/docs/grafana/latest/administration/provisioning/)。RackNerd 具体实现可能与通用 SolusVM 文档有所差异。
