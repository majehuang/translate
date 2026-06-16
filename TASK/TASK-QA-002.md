# TASK-QA-002 · 阶段 M1 验证 Gemini 链路（测试）

> QA Agent（Claude Sonnet 子代理）执行。依据 `TASK/delivery/DELIVERY-002.md` 与本文件验证 M1。规范见 `qa.md`。

## 验收门槛
无 P0/P1/P2 即通过（真实 API 联调项另列为"需人工验证"，不计入自动 Bug 门槛，但须如实记录状态）。

## 必跑命令（贴真实输出）
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p cli
cargo run -p cli            # 应打印输入/输出设备列表
```

## 用例清单

| # | 用例 | 期望 |
|---|---|---|
| 1 | 协议 serde camelCase | `setup.generationConfig.responseModalities[0]=="AUDIO"`；realtimeInput.audio.mimeType 正确 |
| 2 | 容忍未知字段 | 含 usageMetadata/turnComplete 的 JSON 不解析失败 |
| 3 | 编码往返 | `encode_input` → base64 解码 → `from_le_bytes` 还原原帧 |
| 4 | 解码音频 | serverContent 内 inlineData → 正确 i16 样本与 24k 采样率；无内容时为空 |
| 5 | Session mock 闭环 | 客户端发 setup、收到 mock 音频帧 [1,2]@24k、可上行 |
| 6 | 过期帧丢弃 | 10 帧 keep 3 → 保留最新 3（样本 7,8,9） |
| 7 | 重采样比例 | 48k→16k：480→~160（±8）；24k→48k：480→~960（±16） |
| 8 | CPAL 列举不崩 | `list_inputs/list_outputs` 无声卡也不 panic |
| 9 | CLI 列设备 | 无参运行打印输入/输出设备并提示指定设备 |
| 10 | auto 模式 | `Setup::new_translate` 序列化不含 source 字段 |

## 架构纪律核查
- [ ] `engine`/`cli` 无 `#[cfg(target_os)]`；平台差异只在 `audio-cpal`。
- [ ] Session 收/发为独立 task；过期帧丢弃逻辑方向正确（丢旧留新）。
- [ ] `GEMINI_API_KEY` 走环境变量，无硬编码密钥。

## 待人工验证项（Task 13，记录状态即可）
- [ ] 真实 Gemini 协议字段与 `protocol.rs` 一致性。
- [ ] 中↔英真机听感、首包延迟、10 分钟稳定性、P50/P95。
- [ ] `docs/superpowers/notes/M1-validation.md` 是否就位（可为待填模板）。

## 报告
生成 `TASK/reports/QA-REPORT-002-r{round}.md`（见 `qa.md §5`）。含 P0/P1/P2 则打回 Dev。
