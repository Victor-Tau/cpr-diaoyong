# 订阅权重编排

Codex Proxy RS `3.54.x` 插件，要求宿主版本至少为 `3.54.3`。使用管理扩展 v2 和宿主内部 HTTP 分派，无需管理 API Key。

## 安装和使用

1. 在「插件管理」上传 `dist/` 中与服务器平台对应的 `.tar.gz`，确认安装。本地提供的包适用于 Linux x86_64（glibc 2.34 或更新版本）。
2. 打开「订阅权重编排」，选择账号分组和套餐，点击「查询」。默认套餐是 Plus，默认按本期订阅开始时间从早到晚排序，开始时间相同则比较到期时间。
3. 核对订阅开始、到期以及当前和建议权重。取消勾选本次不更新的账号，再点「应用」。也可以选择「到期时间优先」后重新查询。

有效订阅按序建议 `100、99、98…`，完全并列共享权重，有效订阅的建议权重最低为 `2`，已知本期结束的账号建议权重为 `1`。查询失败、日期不足、账号停用或不支持订阅查询时，预览显示原因并跳过写入。日期取自 OpenAI 返回的实际订阅周期，显示沿用宿主时区。

预览最长有效 10 分钟。应用前会复核账号快照和有效期，过期或检测到账号变更时需重新查询。每个权重的账号分批更新，只提交权重字段。若应用中断或部分成功，重新查询即可核对已写入的权重。

账号权重是宿主的全局账号属性，该账号所属的其他分组也会使用新权重。续订后可重新查询并应用。

## 插件设置

| 配置 | 默认值 | 用法 |
| --- | --- | --- |
| `groupIds` | 空 | 限定可选择的分组 ID，空值显示所有启用分组 |
| `planTypes` | `plus` | 可选套餐标识，例如 `plus,pro` |
| `excludedAccountIds` | 空 | 持续跳过指定的 CPR 账号 ID，从名单移除即可恢复参与 |

列表可用逗号分隔。也可在每次预览临时调整要应用的账号，页面会保留这些账号在当前预览中显示的建议值。

## 构建

需要 Rust `1.99.1`、Node.js 24 或更新版本，以及与目标宿主版本匹配的 [`cpr-plugin`](https://github.com/zyycn/codex-proxy-rs/blob/6900da36eedb3099d43c8c9bf5b8b97d82d15688/backend/apps/plugin-cli/README.md)。在此目录运行：

```bash
npm --prefix frontend ci
npm --prefix frontend run build
cargo +1.99.1 test --locked
cargo +1.99.1 build --release --locked --target x86_64-unknown-linux-gnu
cpr-plugin package \
  --manifest plugin.json \
  --binary target/x86_64-unknown-linux-gnu/release/cpr-diaoyong \
  --target x86_64-unknown-linux-gnu \
  --resource-map web=frontend/dist \
  --output-dir dist
```
