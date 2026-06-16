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

- `serverContent.modelTurn.parts.inlineData` 字段是否与 `protocol.rs` 一致：
- 如不一致，差异与修正：
- 是否出现解析失败帧：

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
