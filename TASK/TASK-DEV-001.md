# TASK-DEV-001 · 阶段 M0 地基（编码）

> Dev Agent（Codex CLI）执行。权威实现细节见 `docs/superpowers/plans/2026-06-13-m0-foundation-m1-gemini-link.md` 的 **Task 1–5**（含每个文件完整代码、测试、命令）。本文件给范围与验收，**代码以计划文档为准**。

## 目标
搭起 Cargo workspace 地基：核心音频类型、无锁环形缓冲、`AudioBackend` trait、engine 控制面状态机，双平台 CI 立起来，`cargo test --workspace` 全绿。

## 工作内容（对应计划 Task 1–5）

- **Task 1 · workspace 与双平台 CI**
  - `Cargo.toml`（workspace 根，workspace.dependencies 见计划）、`rust-toolchain.toml`（1.83）、`.gitignore`、`.github/workflows/ci.yml`（macOS + Windows：fmt + clippy + test）。
  - 占位 crate `crates/audio-core`（Cargo.toml + lib.rs）。
  - 验证：`cargo build --workspace` 成功。

- **Task 2 · `audio-core::PcmFrame`**
  - `frame.rs`：`PcmFrame{samples,sample_rate}`、`duration_ms`、`to_le_bytes`/`from_le_bytes`。
  - 测试：`duration_of_16k_frame`、`le_bytes_roundtrip`。

- **Task 3 · 无锁环形缓冲**
  - `ring.rs`：`audio_channel(capacity)` → `AudioProducer/AudioConsumer`，`push_slice`/`pop_slice`，满时丢旧不阻塞。
  - 测试：`push_then_pop_roundtrip`、`push_drops_overflow_without_blocking`。

- **Task 4 · `AudioBackend` trait**
  - `backend.rs`：`DeviceId/DeviceInfo/StreamCfg`、`InputStream/OutputStream/AudioBackend` trait、`is_virtual_device_name`。
  - 测试：`detects_virtual_devices_by_name`。

- **Task 5 · engine 控制面**
  - `crates/engine`：`TranslateMode`、`SourceLang`、`SessionState`、`ControlEvent`、`worst_state` 最坏态投影。
  - 测试：`worst_picks_error_over_running`、`worst_picks_reconnecting_over_running`、`both_running_is_running`。

## 验收标准（M0 检查点）
- [ ] `cargo build --workspace` 成功。
- [ ] `cargo test --workspace` 全绿（audio-core 5 + engine 3 = 8 个测试）。
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 无警告。
- [ ] `cargo fmt --all -- --check` 通过。
- [ ] 接口签名与计划一致（`PcmFrame`/`AudioBackend`/`worst_state` 等）。
- [ ] `engine` 内无 `#[cfg(target_os)]`。

## 交付
完成后按 `AGENTS.md §6` 生成 `TASK/delivery/DELIVERY-001.md`。
