# 交付文档 DELIVERY-001（阶段：M0）

## 1. 范围

本次完成 `TASK/TASK-DEV-001.md` 中 M0 Task 1-5：

- Task 1：建立 Cargo workspace、Rust 1.83 toolchain、双平台 CI、`audio-core` 占位 crate。
- Task 2：实现 `audio-core::PcmFrame`、帧时长计算、小端 PCM 字节转换。
- Task 3：实现 `audio-core` 无锁环形缓冲封装 `audio_channel`、`AudioProducer`、`AudioConsumer`。
- Task 4：实现平台无关 `AudioBackend` trait、设备类型、流配置和虚拟设备名识别。
- Task 5：实现 `engine` 控制面消息、Session 子状态和 `worst_state` 最坏态投影。

## 2. 改动清单

- `Cargo.toml`：定义 workspace、共享依赖和 resolver。
- `Cargo.lock`：锁定当前 workspace 依赖版本，实际使用 `ringbuf 0.4.8`。
- `rust-toolchain.toml`：锁定 Rust 1.83，并声明 clippy/rustfmt 组件。
- `.gitignore`：忽略 `target`、备份文件、`.env` 和日志。
- `.github/workflows/ci.yml`：在 macOS 与 Windows 上运行 fmt、clippy、test。
- `crates/audio-core/Cargo.toml`：定义 `audio-core` crate 及 `thiserror`、`ringbuf` 依赖。
- `crates/audio-core/src/lib.rs`：导出音频核心类型、环形缓冲和后端 trait。
- `crates/audio-core/src/frame.rs`：实现 `PcmFrame` 与小端 PCM 编解码测试。
- `crates/audio-core/src/ring.rs`：封装 ringbuf 0.4 producer/consumer，保证回调侧 push 不阻塞。
- `crates/audio-core/src/backend.rs`：定义音频后端抽象和虚拟设备识别。
- `crates/engine/Cargo.toml`：定义 `engine` crate 及对 `audio-core` 的路径依赖。
- `crates/engine/src/lib.rs`：导出控制面模块。
- `crates/engine/src/control.rs`：实现控制面状态枚举、事件和最坏态投影。

## 3. 自测结果

- `cargo fmt --all -- --check`：通过，退出码 0。

关键输出：

```text
<no output>
```

- `cargo clippy --workspace --all-targets -- -D warnings`：通过，退出码 0。

关键输出：

```text
Checking audio-core v0.1.0 (/Users/maje/Workspace/translate/crates/audio-core)
Checking engine v0.1.0 (/Users/maje/Workspace/translate/crates/engine)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.27s
```

- `cargo test --workspace`：通过，8 passed。

逐 crate 测试数：

- `audio-core`：5 passed；0 failed。
- `engine`：3 passed；0 failed。
- `audio_core` doctest：0 passed；0 failed。
- `engine` doctest：0 passed；0 failed。

关键输出：

```text
running 5 tests
test backend::tests::detects_virtual_devices_by_name ... ok
test frame::tests::duration_of_16k_frame ... ok
test frame::tests::le_bytes_roundtrip ... ok
test ring::tests::push_drops_overflow_without_blocking ... ok
test ring::tests::push_then_pop_roundtrip ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 3 tests
test control::tests::both_running_is_running ... ok
test control::tests::worst_picks_reconnecting_over_running ... ok
test control::tests::worst_picks_error_over_running ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

补充验收：

- `cargo build --workspace`：通过，退出码 0。
- `rg "target_os" crates/engine crates/audio-core Cargo.toml .github/workflows/ci.yml`：仅在 `audio-core/src/backend.rs` 文档注释中出现纪律说明，`engine` 中无 `#[cfg(target_os)]`。

## 4. 与计划的差异

- Task 3 的 TASK 描述写为“满时丢旧不阻塞”，但 `ringbuf 0.4.8` 中普通 `Producer::push_slice` 的实际语义是只写入可容纳的前缀，返回实际写入数；溢出的新样本不会写入，也不会覆盖旧数据。
- `ringbuf 0.4.8` 另有 owning ring buffer 上的 `push_slice_overwrite`，但当前封装持有的是 split 后的 producer/consumer。为保持音频回调线程无阻塞、无额外同步、低风险，本阶段采用“满时丢弃溢出新样本，绝不阻塞回调”的安全语义，并已在 `ring.rs` 文档注释和测试中明确。
- Task 3 commit message 因此使用“满时丢弃溢出”而非“满时丢旧”。
- `Cargo.lock` 在 Task 5 提交，用于锁定实际依赖解析结果；当前锁定 `ringbuf 0.4.8`、`thiserror 2.0.18`。

## 5. 已知限制 / 待 QA 验证项

- M0 仅提供平台无关核心抽象和状态机，没有真实音频设备接入、真实 API 链路、延迟指标或听感验证。
- 双平台 CI 配置已生成，但本地只在当前 macOS 环境执行了自测；Windows 与 GitHub Actions 结果需 QA/CI 验证。
- `AudioBackend` 目前是 trait 合约，没有 CPAL 后端实现；真实设备枚举、采集和播放需 M1 后续 Task 验证。
- 环形缓冲当前语义为“满时丢弃溢出新样本、不阻塞”，不是覆盖旧样本；后续若产品必须保留最新音频，应在设计确认后更换为可证明无锁/低风险的覆盖策略。

## 6. QA 切入点

- 优先运行：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- 单独验证 Task 3：

```bash
cargo test -p audio-core ring
```

- 单独验证控制面：

```bash
cargo test -p engine control
```

- 核查双平台纪律：

```bash
rg "target_os" crates/engine crates/audio-core Cargo.toml .github/workflows/ci.yml
```

- 核查提交粒度：

```bash
git log --oneline -5
```
