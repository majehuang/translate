# 测试报告 QA-REPORT-002-r1

## 1. 概述

- 阶段：M1（验证 Gemini 链路）
- 轮次：r1
- 被测交付文档：`TASK/delivery/DELIVERY-002.md`（commit `2aff855` 及之前的 M1 一系列 commit）
- 工具链：rustc/cargo 1.83.0（与 `rust-toolchain.toml` 一致）
- 结论：**通过**（无 P0/P1/P2；发现 1 个 P3）

## 2. 命令执行结果

```text
$ cargo fmt --all -- --check
EXIT_CODE=0   # 无输出，格式全部符合

$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.22s
EXIT_CODE=0   # 无任何 warning

$ cargo test --workspace
audio-core:    5 passed
audio-cpal:    1 passed
audio-dsp:     2 passed
cli:           0 passed
engine:        3 passed
gemini-live:   9 passed (unit)
gemini-live:   1 passed (tests/session_mock.rs 集成测试)
doc-tests:     0 (全部空)
合计：21 passed; 0 failed; 0 ignored
EXIT_CODE=0
```

与 DELIVERY-002 §3 自报数字（21 passed）一致，独立复测吻合，未发现编造。

```text
$ cargo build -p cli
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.06s
EXIT_CODE=0

$ cargo run -p cli   （无参，连续跑 2 次确认稳定）
== 输入设备 ==
  外置麦克风 [默认]
== 输出设备 ==
  外置耳机 [默认]

请用 --in-device/--out-device 指定设备后重跑。
EXIT_CODE=0，未 panic
```

说明：本 QA 机器 CPAL 枚举本身可在 2 秒超时阈值内返回真实设备（外置麦克风/外置耳机），与 DELIVERY-002 中记录的"枚举超时返回空列表"的 Dev 机器现象不同，属正常的环境差异，不影响"无参不崩溃且给出指引"的验收点。

mock 集成测试 `session_mock.rs` 额外重复执行 3 次确认无 flaky（均 0.00s 内通过）。

## 3. 用例结果

| # | 用例 | 期望 | 实际 | 结论 |
|---|---|---|---|---|
| 1 | 协议 serde camelCase | `setup.generationConfig.responseModalities[0]=="AUDIO"`；`realtimeInput.audio.mimeType` 正确 | `protocol.rs::setup_serializes_with_camel_case` 断言 `json["setup"]["model"]` 与 `json["setup"]["generationConfig"]["responseModalities"][0]=="AUDIO"`；`realtime_input_serializes` 断言 `json["realtimeInput"]["audio"]["mimeType"]=="audio/pcm;rate=16000"` 与 `data`。两测试均真实执行 serde_json 序列化后做字段级断言，非字符串包含式弱断言 | 通过 |
| 2 | 容忍未知字段 | 含 `usageMetadata`/`turnComplete` 的 JSON 不解析失败 | `server_message_tolerates_unknown_fields` 用 `{"serverContent":{"turnComplete":true},"usageMetadata":{"x":1}}` 反序列化，`unwrap()` 不 panic 即测试通过；`ServerContent`/`ServerMessage` 均未对未知字段设 `deny_unknown_fields`，结构性保证容忍 | 通过 |
| 3 | 编码往返 | `encode_input` → base64 解码 → `from_le_bytes` 还原原帧 | `encode_then_manual_decode_roundtrip` 真实跑通编码→标准 base64 解码→`PcmFrame::from_le_bytes`，并 `assert_eq!` 还原帧与原帧（含负数 -1/-100 样本）完全相等 | 通过 |
| 4 | 解码音频 | `serverContent` 内 `inlineData` → 正确 i16 样本与 24k 采样率；无内容时为空 | `decode_audio_extracts_frame` 用真实 base64 `AQACAA==` 断言解出 `[1,2]@24000Hz`；`decode_audio_empty_when_no_content` 用 `ServerMessage::default()` 断言返回空 Vec | 通过 |
| 5 | Session mock 闭环 | 客户端发 setup、收到 mock 音频帧 `[1,2]@24k`、可上行 | `session_mock.rs`：mock server 校验首条消息含 `generationConfig`（验证 setup 真发出且为 camelCase JSON），回一帧音频，客户端 `rx.recv()` 在 2s 超时内拿到 `samples==[1,2]`、`sample_rate==24000`；随后 `tx.send(...)` 验证上行不阻塞/不报错。闭环断言完整，非仅"能连上" | 通过 |
| 6 | 过期帧丢弃 | 10 帧 keep 3 → 保留最新 3（样本 7,8,9） | `drop_stale_keeps_latest` 构造 0..10 帧，`drop_stale_frames(&mut q,3)` 后断言 `q.len()==3`、`q[0].samples[0]==7`、`q[2].samples[0]==9`；另有 `drop_stale_noop_when_under_limit` 覆盖未超限分支 | 通过 |
| 7 | 重采样比例 | 48k→16k：480→~160（±8）；24k→48k：480→~960（±16） | `downsample_48k_to_16k_ratio`、`upsample_24k_to_48k_ratio` 均用真实 `rubato::FftFixedIn` 处理 480 样本输入，断言输出长度落在容差区间内，且 `sample_rate` 字段正确 | 通过 |
| 8 | CPAL 列举不崩 | `list_inputs`/`list_outputs` 无声卡也不 panic | `backend_lists_without_panicking` 调用两接口仅判断不 panic（弱断言，未对返回值做内容校验，但用例本身就是"不崩"，符合期望）；本机实测列出真实设备且全程无 panic | 通过 |
| 9 | CLI 列设备 | 无参运行打印输入/输出设备并提示指定设备 | 实测 `cargo run -p cli` 打印 `== 输入设备 ==`/`== 输出设备 ==` 与具体设备名，并打印"请用 --in-device/--out-device 指定设备后重跑。"，`EXIT_CODE=0` | 通过 |
| 10 | auto 模式 | `Setup::new_translate` 序列化不含 `source` 字段 | **见 Bug-001**：`SetupBody`/`GenerationConfig` 结构体定义中本就没有任何 `source`/`sourceLanguageCode` 字段，序列化自然不会出现，行为正确；但现有测试 `setup_serializes_with_camel_case` 未对此显式断言（如 `assert!(json["setup"].get("source").is_none())`），用例期望与现状之间缺一条直接断言。判定为通过（行为正确），但记为 P3 改进项 | 通过（行为正确，断言覆盖有缺口） |

## 4. 架构纪律核查

- [x] `engine`/`cli` 无 `#[cfg(target_os)]`：全仓库 `grep -rn "cfg(target_os"` 仅在 `audio-core/src/backend.rs` 的**注释**里提到这条规则本身，源码中无任何实际 `#[cfg(target_os)]` 使用，平台差异确实只体现在 `audio-cpal`（通过 `cpal` crate 抽象，未见直接 OS 分支）。
- [x] Session 收/发为独立 task：`session.rs::connect` 中 `tokio::spawn` 两次，一个专职从 `audio_in` 收 PcmFrame 编码后写 WS（发送方向），另一个专职从 WS 读消息解码后转发到 `audio_out`（接收方向），两者完全独立、互不阻塞。
- [x] 过期帧丢弃逻辑方向正确（丢旧留新）：`drop_stale_frames` 用 `queue.drain(0..drop_n)` 丢弃前 `drop_n` 个（最旧），保留尾部最新 `keep` 个，测试 `drop_stale_keeps_latest` 验证保留的是样本值 7/8/9（最新），方向正确。
- [x] `GEMINI_API_KEY` 走环境变量，无硬编码密钥：`cli/src/main.rs` 用 `std::env::var("GEMINI_API_KEY")`，全仓库 grep 未发现任何硬编码 key/token 字面值（如 `AIza...`、`sk-...`）。

额外核查（数据面回调零分配/零 await/零锁）：
- `audio-cpal/src/lib.rs` 的 `build_input_stream`/`build_output_stream` 回调内部仅做单样本转换 + 环形缓冲单样本 `push_slice(&[mono])`/`pop_slice(&mut mono)`，无 `Vec::new`/`vec!`/`.clone()`/`Mutex`/`.await`，比计划草稿（曾用 `Vec::with_capacity` 下混）更严格，符合"音频回调零分配"纪律。

## 5. Bug 清单

| 编号 | 级别 | 标题 | 复现步骤 | 期望 vs 实际 | 定位 |
|---|---|---|---|---|---|
| BUG-001 | P3 | auto 模式"不含 source 字段"缺少显式序列化断言 | 阅读 `crates/gemini-live/src/protocol.rs` 的 `setup_serializes_with_camel_case` 测试 | 期望：测试显式断言 `json["setup"].get("source").is_none()` 或等价表达，直接覆盖 TASK-QA-002 用例 10 的验收点；实际：测试只断言 `model` 与 `responseModalities[0]`，未断言 source 字段缺失。当前行为正确（`SetupBody` 结构体本身无该字段，序列化天然不会产生），但属于"测试名义覆盖、断言未落地"的缺口，建议补一行断言以防未来给 `SetupBody` 加 `source` 字段时悄悄回归 | `crates/gemini-live/src/protocol.rs:100-108`（`setup_serializes_with_camel_case` 测试体） |

无 P0/P1/P2。

## 6. 待人工验证项（Task 13）状态

| 项 | 状态 |
|---|---|
| 真实 Gemini 协议字段与 `protocol.rs` 一致性 | **需人工验证** — 无真实 `GEMINI_API_KEY`，本轮未能连接真实服务，无法核对真实 `serverContent`/`inlineData` 字段命名是否与 `protocol.rs` 完全一致 |
| 中↔英真机听感、首包延迟、10 分钟稳定性、P50/P95 | **需人工验证** — 无真实麦克风/扬声器音频回路与真实 API key，无法在自动化环境中复现端到端听感与延迟统计 |
| `docs/superpowers/notes/M1-validation.md` 是否就位 | **已就位（待填模板）** — 文件存在，结构覆盖环境信息、协议核对、中→英/英→中听感、10 分钟稳定性与延迟、PRD 指标校准、结论共 7 节，所有数据字段均为空白待填，文档自身在开头明确标注"状态：需人工/QA 联调，未自动验证"，未发现臆造数据 |

以上三项按 qa.md 规则不计入自动 Bug 门槛，仅如实记录状态，不影响本轮"通过"结论。

## 7. 结论

- 是否存在 P0/P1/P2：**否**。三连命令（fmt/clippy/test）真实复测全绿，`cargo build -p cli` 与 `cargo run -p cli` 真实执行无 panic 且打印设备列表，10 条用例逐条核对断言后均判定通过（含 1 处断言覆盖缺口降级为 P3，不影响功能正确性），架构纪律 4 项核查全部满足。
- **结论：通过，可进入下一阶段。**
- 轮次计数：r1（本轮即通过，未触发"≥5 轮仍有 P2+ 需人工介入"条款）。
- 遗留改进项（不阻断）：BUG-001（P3，建议 Dev 在下一轮顺手补一条 source 字段缺失的显式断言）；Task 13 三项人工联调待真实 `GEMINI_API_KEY` 与真实音频设备后由人工/QA 在目标机器上补测。
