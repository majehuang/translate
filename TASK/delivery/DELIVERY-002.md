# 交付文档 DELIVERY-002（阶段：M1）

## 1. 范围

本次实现 M1 Task 6-13：

- Task 6：`gemini-live` BidiGenerateContent 协议 serde 类型，auto 模式 `Setup::new_translate` 不写 source。
- Task 7：`PcmFrame` 与 Gemini realtime/audio part 的 base64 + little-endian PCM 编解码。
- Task 8：`Session` WebSocket setup、发送、接收闭环，并用本地 mock WS 集成测试覆盖。
- Task 9：过期帧丢弃 `drop_stale_frames` 与指数退避 `connect_with_retry`。
- Task 10：`audio-dsp` 基于 rubato 的 48k/16k/24k 重采样器。
- Task 11：`audio-cpal` 后端，支持设备枚举、输入采集、输出播放和环形缓冲推拉。
- Task 12：`cli` 单向链路：麦克风 -> 16k -> Gemini -> 24k -> 扬声器；无参列设备。
- Task 13：创建真实 API/真实设备人工验收记录模板。

## 2. 改动清单

- `crates/gemini-live/Cargo.toml`：新增 Gemini Live 客户端 crate 依赖。
- `crates/gemini-live/src/protocol.rs`：协议 serde 类型与协议序列化/反序列化测试。
- `crates/gemini-live/src/codec.rs`：音频帧编码为 realtimeInput、服务端音频 part 解码为 `PcmFrame`。
- `crates/gemini-live/src/session.rs`：WebSocket 连接、setup、收发 task、过期帧丢弃与重连封装。
- `crates/gemini-live/tests/session_mock.rs`：本地 mock WebSocket 收发闭环集成测试。
- `crates/audio-dsp/Cargo.toml`：新增 DSP crate 依赖。
- `crates/audio-dsp/src/resample.rs`：rubato 定长块重采样器与比例测试。
- `crates/audio-dsp/src/lib.rs`：导出重采样器。
- `crates/audio-cpal/Cargo.toml`：新增 CPAL 后端 crate 依赖。
- `crates/audio-cpal/src/lib.rs`：实现 `AudioBackend` 的 CPAL 输入/输出后端，设备枚举带超时保护。
- `crates/cli/Cargo.toml`：新增 M1 CLI crate 依赖。
- `crates/cli/src/main.rs`：实现无参设备列表和有参单向实时翻译链路。
- `docs/superpowers/notes/M1-validation.md`：真实 API/麦克风/扬声器人工验收模板，明确未自动验证。
- `Cargo.lock`：锁定 M1 新依赖，并固定 Rust 1.83 兼容的间接依赖版本。

## 3. 自测结果

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `cargo test --workspace`：通过，合计 21 passed。

逐 crate 测试数：

- `audio-core`：5 passed。
- `audio-cpal`：1 passed。
- `audio-dsp`：2 passed。
- `cli`：0 passed。
- `engine`：3 passed。
- `gemini-live`：9 unit passed + 1 integration passed。

额外验收：

- `cargo build -p cli`：通过。
- `cargo run -p cli`：通过；当前环境 CPAL 输入/输出枚举均超时后返回空列表并正常提示指定设备，未 panic。

关键输出：

```text
cargo clippy --workspace --all-targets -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s)

cargo test --workspace
test result: ok. 21 passed; 0 failed

cargo run -p cli
== 输入设备 ==
WARN audio_cpal: CPAL 设备枚举超时，返回空列表 direction="input"
== 输出设备 ==
WARN audio_cpal: CPAL 设备枚举超时，返回空列表 direction="output"
请用 --in-device/--out-device 指定设备后重跑。
```

## 4. 与计划的差异

- `zeroize`：`tokio-tungstenite` 的 rustls 依赖链默认解析到 `zeroize 1.9.0`，其 manifest 需要 Cargo edition2024；本仓库工具链为 1.83，因此锁定为 `zeroize 1.8.2`。
- `coreaudio-sys`：`cpal 0.15.3` 在 macOS 依赖链默认解析到 `coreaudio-sys 0.2.18`，其 manifest 需要 Cargo edition2024；本仓库工具链为 1.83，因此锁定为 `coreaudio-sys 0.2.16`，同时带来 `bindgen 0.70.1` 等兼容降级。
- `tokio-tungstenite 0.24`：本次解析到的 `Message::Text` 入参仍为 `String`，计划中的 `.into()` 会触发 clippy `useless_conversion`，已移除。
- `audio-cpal`：计划代码在音频回调中为下混/播放临时分配 `Vec`；本实现改为逐样本转换并通过环形缓冲单样本 push/pop，避免回调内 heap 分配。
- `audio-cpal` 设备枚举：当前本机 CPAL 枚举会阻塞超过 60 秒；为保证自动化和 CLI 无参模式不挂死，`list_inputs/list_outputs` 增加 2 秒超时保护，超时返回空列表并记录 warning。

## 5. 已知限制 / 待 QA 验证项

- Task 13 需人工/QA 联调，未自动验证：需要真实 `GEMINI_API_KEY`、麦克风和扬声器，验证中英/英中听感、协议字段、10 分钟稳定性和 P50/P95 延迟。
- 当前自动化未验证真实 Gemini 服务连接、真实音频输入输出、首包延迟和连续运行听感。
- 当前环境 CPAL 设备枚举超时，CLI 无参能安全退出，但真实设备列表需在 QA 机器上复测。
- 单向 CLI 链路是 M1 验证链路；双向链路、虚拟设备路由、VAD、AEC、UI 不在本阶段范围。

## 6. QA 切入点

- 运行自动化门禁：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p cli
cargo run -p cli
```

- 使用真实设备和 API key 验证：

```bash
GEMINI_API_KEY=<key> cargo run -p cli -- --in-device "<麦克风>" --out-device "<扬声器>" --target en
GEMINI_API_KEY=<key> cargo run -p cli -- --in-device "<麦克风>" --out-device "<扬声器>" --target zh
```

- 按 `docs/superpowers/notes/M1-validation.md` 填写 Task 13 结果，重点检查：
  - 真实服务端帧字段是否仍匹配 `protocol.rs`。
  - 中文到英文、英文到中文是否有可听译文。
  - 是否有爆音、卡顿、延迟持续堆积。
  - 连续 10 分钟是否崩溃或断线。
  - 首包延迟、逐句 P50/P95 是否需要反向调整 PRD 指标。
