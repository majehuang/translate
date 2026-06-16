# TASK-DEV-002 · 阶段 M1 验证 Gemini 链路（编码）

> Dev Agent（Codex CLI）执行。权威实现细节见 `docs/superpowers/plans/2026-06-13-m0-foundation-m1-gemini-link.md` 的 **Task 6–13**。本文件给范围与验收，**代码以计划文档为准**。

## 前置
M0 已通过 QA。

## 目标
自实现 `gemini-live` WebSocket 客户端（最大单点风险），接通 CPAL 音频 I/O + rubato 重采样，交付命令行单向实时翻译链路（麦→16k→Gemini→24k→扬声器），双平台可编译运行。

## 工作内容（对应计划 Task 6–13）

- **Task 6 · gemini-live 协议 serde**：`protocol.rs`（Setup/RealtimeInput/ServerMessage…，camelCase，容忍未知字段）。测试 4 个。
- **Task 7 · 音频编解码**：`codec.rs`（`encode_input`/`decode_audio`，base64 + LE PCM）。测试 3 个。
- **Task 8 · Session 收发闭环**：`session.rs` `connect()`（setup→收发 task）+ `tests/session_mock.rs`（本地 mock WS）。测试 1 个。
- **Task 9 · 重连与过期帧丢弃**：`drop_stale_frames`、`connect_with_retry`（指数退避）。测试 2 个。
- **Task 10 · audio-dsp 重采样**：`resample.rs` `Resampler`（rubato `FftFixedIn`，48k↔16k/24k）。测试 2 个。
- **Task 11 · audio-cpal 后端**：`CpalBackend` 实现 `AudioBackend`（双平台输入输出，回调推/拉环形缓冲）。测试 1 个（不 panic）。
- **Task 12 · CLI 链路**：`crates/cli/src/main.rs` 串起单向链路，列设备模式 + 运行模式，记录首包延迟。
- **Task 13 · 真实 API 联调（手动）**：协议核对、中↔英听感、10 分钟稳定性 + P50/P95，写 `docs/superpowers/notes/M1-validation.md`。**此项需真实 GEMINI_API_KEY + 麦克风/扬声器，标注"需人工/QA 联调"，不得伪造。**

## 验收标准（M1 检查点）
- [ ] `cargo test --workspace` 全绿（新增 gemini-live ~10、audio-dsp 2、audio-cpal 1）。
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 无警告。
- [ ] `cargo fmt --all -- --check` 通过。
- [ ] `cargo build -p cli` 成功；`cargo run -p cli`（无参）打印设备列表。
- [ ] auto 模式：`Setup::new_translate` 不写 source；CLI `--target` 设目标语言。
- [ ] 过期帧丢弃保留最新、重连指数退避实现存在。
- [ ] Task 13 真实联调项明确标注为待人工/QA 验证（不阻塞编译/单测验收）。

## 交付
完成后按 `AGENTS.md §6` 生成 `TASK/delivery/DELIVERY-002.md`，并把 Task 13 列入"待 QA 验证项"。
