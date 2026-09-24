# Product

## Register

product

## Users

现有监控服务的维护者与获准访问内网 Grafana 的匿名只读用户。

## Product Purpose

在既有 Grafana 中汇总 RackNerd 服务器账期流量，以及 sing-box 用户用量和速度。新增仪表盘作为默认首页，沿用已有 Prometheus 数据源。

## Brand Personality

沿用现有 Grafana 仪表盘：清晰、克制、准确。中文面板标题，标准图表、表格、筛选与单位。

## Anti-references

项目当前不包含独立前端应用；不新增另一套监控服务，不修改 Grafana 的全局视觉风格。

## Design Principles

- 先查看服务器剩余额度，再查看用户用量和速度。
- RackNerd 与 sing-box 分别选择数据源中的服务器和实例，不假定它们是同一台机器。
- 区分账期统计、所选时间段增量和近一分钟平均速度。
- 用户标签为空的流量显示为“未归属”，缺失数据不填为零。
- 复用 Grafana 的主题、交互、语义颜色与权限。

## Accessibility & Inclusion

沿用 Grafana 内建的键盘操作、表格和主题。状态同时使用文字和颜色；未提出额外品牌或专项无障碍要求。
