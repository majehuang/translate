# M1 验收记录

> 状态：需人工/QA 联调，未自动验证。
> 原因：本项需要真实 `GEMINI_API_KEY`、麦克风和扬声器，并需要人工确认翻译听感、稳定性和延迟。

## 1. 环境

- 日期：
- 平台（macOS / Windows）：
- 设备：
  - 输入设备：
  - 输出设备：
- 模型 ID：`models/gemini-3.5-live-translate`
- target 语言：
- 命令：

```bash
GEMINI_API_KEY=<key> cargo run -p cli -- --in-device "<麦克风>" --out-device "<扬声器>" --target en
```

## 2. 协议核对

> 状态：**已用 `cargo run -p gemini-live --example smoke` 对真实 API 实测确认**（2026-06-16，无需麦克风，发送静音核对握手与 setup 契约）。结果：收到 `{"setupComplete":{}}`。

实测确认并已修正进代码的契约：

| 项 | 计划/旧代码（错误） | 真实 API（已修正） |
|---|---|---|
| 模型 ID | `models/gemini-3.5-live-translate` | `models/gemini-3.5-live-translate-preview` |
| 鉴权 query | `?access_token=<key>` | `?key=<key>`（API Key；access_token 报 unregistered callers） |
| 目标语言位置 | `setup.targetLanguageCode`（或缺失） | `setup.generationConfig.translationConfig.{targetLanguageCode, echoTargetLanguage}` |
| TLS | rustls 无 crypto provider → 握手 panic | 安装 ring provider（`ensure_crypto_provider`） |
| `setupComplete` | `ServerMessage.setup_complete` | 一致 ✅ |

- `serverContent.modelTurn.parts.inlineData`（译文音频帧）：**待补**——静音输入不产生译文音频，需真人说话时核对（见 §3/§4）。
- 是否出现解析失败帧：握手与 setup 阶段无解析失败。
- 网络备注：本机经本地代理（DNS → 198.18.0.97 fake-IP），偶发 `tls handshake eof`，重试即成功。

## 3. 中文到英文听感

- target：`en`
- 是否听到英文翻译：
- 是否有爆音：
- 是否有卡顿：
- 是否有延迟持续堆积：
- 备注：

## 4. 英文到中文听感

- target：`zh`
- 是否听到中文翻译：
- 是否有爆音：
- 是否有卡顿：
- 是否有延迟持续堆积：
- 备注：

## 5. 10 分钟稳定性与延迟

- 运行时长：
- 是否崩溃：
- 是否断线重连：
- 首包延迟（ms）：
- 逐句 P50（ms）：
- 逐句 P95（ms）：
- 原始日志路径：

## 6. PRD 指标校准

- 与 PRD 9.1 延迟指标的差距：
- 是否建议放宽指标：
- 建议的新指标：

## 7. 结论

- QA 结论：
- 阻塞问题：
- 后续建议：
