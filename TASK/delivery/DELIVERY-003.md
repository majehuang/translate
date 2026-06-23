# 交付文档 DELIVERY-003（阶段：M2）

## 1. 范围

本次实现 M2 Task 14-26：

- Task 14：`audio-core` 扩展设备方向、原始设备事件、设备 watch 句柄与 `AudioBackend::watch_devices`。
- Task 15-17：新增 `device-manager`，实现设备用途分类、快照 diff、DeviceLost 投影与 `DeviceManager` 不可变刷新查询。
- Task 18-20：新增 `diagnostics`，实现逐帧能量摘要、物理隔离校验、回环检测与滞回暂停/恢复状态机。
- Task 21：`audio-dsp` 新增能量 + 过零率 VAD 与 `VadStats`。
- Task 22：`audio-cpal` 实现轮询式 `watch_devices`，带空列表保护。
- Task 23-25：`engine` 扩展 Paused 控制面、路由矩阵、隔离委托、真实单链路接线与双链路编排器。
- Task 26：CLI 升级为 engine 薄壳，支持三态模式、双链路设备参数、分类设备表、隔离校验和 M2 人工验收模板。

## 2. 改动清单

- `crates/audio-core/src/backend.rs`：新增 `Direction`、`RawDeviceEvent`、`DeviceWatchHandle` 与 `watch_devices` trait 方法。
- `crates/audio-core/src/lib.rs`：导出设备 watch 相关类型。
- `crates/device-manager/*`：新增平台无关设备分类、快照 diff、语义事件投影和管理器。
- `crates/diagnostics/*`：新增能量摘要、物理隔离和回环检测纯函数。
- `crates/audio-dsp/src/vad.rs`：新增 VAD 纯函数、状态机和统计。
- `crates/audio-dsp/src/lib.rs`：导出 VAD API。
- `crates/audio-cpal/src/lib.rs`：实现 CPAL 设备热插拔轮询 watch。
- `crates/engine/Cargo.toml`：新增 M2 engine 依赖，不依赖 `audio-cpal`。
- `crates/engine/src/control.rs`：新增 Paused、PauseReason 和防循环控制事件。
- `crates/engine/src/route.rs`：新增三态路由矩阵与隔离校验委托。
- `crates/engine/src/link.rs`：新增单链路真实音频/Gemini 接线。
- `crates/engine/src/orchestrator.rs`：新增双链路状态聚合和独立事件上报。
- `crates/engine/src/lib.rs`：导出 M2 engine 模块。
- `crates/cli/Cargo.toml`：新增 engine/device-manager 依赖。
- `crates/cli/src/main.rs`：改为 M2 双链路编排薄壳。
- `crates/gemini-live/tests/session_mock.rs`：本地 loopback 被沙箱禁止时显式跳过 mock WS 集成测试。
- `docs/superpowers/notes/M2-validation.md`：新增真机/QA 联调记录模板。

## 3. 自测结果

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `cargo test --workspace`：53 passed。

逐 crate 测试数：

- `audio-core`：6 passed。
- `audio-cpal`：2 passed。
- `audio-dsp`：8 passed。
- `cli`：2 passed。
- `device-manager`：5 passed。
- `diagnostics`：6 passed。
- `engine`：13 passed。
- `gemini-live`：10 unit passed + 1 integration passed。

额外验收：

- `cargo build --workspace`：通过。
- `cargo build -p cli`：通过。
- `cargo run -p cli`：通过；当前沙箱环境枚举到的输入/输出设备为空，打印分类设备表头后正常退出，未 panic。
- 架构扫描：`engine` / `device-manager` / `diagnostics` / `cli` 无 `#[cfg(target_os)]`。
- 占位符扫描：`crates/` 下无未实现或待办占位标记。
- 依赖扫描：`device-manager` / `diagnostics` 的 `Cargo.toml` 无 `cpal` / `audio-cpal`。

## 4. 与计划的差异

- `engine::link::spawn_link`：计划示例是骨架占位；本次已实现真实接线（open input/output、Gemini connect_with_retry、VAD、上下行重采样与 ring push/pop），真机效果仍需 QA 联调。
- `diagnostics::detect_loop`：能量比按最佳 lag 对齐后的 RMS 平方均值计算，避免把延迟前静音计入 captured 均值导致比例低估。
- `gemini-live` mock WS 测试：当前沙箱禁止 loopback bind，测试在 `PermissionDenied` 时显式跳过；允许本地监听的 QA/CI 环境仍执行完整 mock WebSocket 闭环。
- `cli` 无参冒烟：当前沙箱 CPAL 枚举为空，因此自动验证到“不 panic + 表头打印”，真实分类标签命中率需 QA 在真机设备上验证。

## 5. 已知限制 / 待 QA 验证项

- 需人工/QA 联调，未自动验证：双向 30 分钟稳定性。
- 需人工/QA 联调，未自动验证：两链路独立断线重连，一条 Reconnecting 不拖停另一条。
- 需人工/QA 联调，未自动验证：`bidirectional` / `uplink-only` / `downlink-only` 三态省成本观测。
- 需人工/QA 联调，未自动验证：真实声学回环下数秒内触发 Paused 和配置修复提示。
- 需人工/QA 联调，未自动验证：防双声主观听感，物理耳机只听译文。
- 需人工/QA 联调，未自动验证：VAD 真机阈值标定、句首句尾不截断、噪声不误触发。
- 需人工/QA 联调，未自动验证：热插拔时序、DeviceLost→Error 子态、插回刷新。
- 需人工/QA 联调，未自动验证：BlackHole/VB-CABLE 等虚拟设备真实命名命中率。

## 6. QA 切入点

- 自动门禁：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p cli
```

- 真机联调示例：

```bash
GEMINI_API_KEY=<key> cargo run -p cli -- \
  --mode bidirectional \
  --uplink-in "<物理麦>" --uplink-out "<虚拟麦克风>" --uplink-target en \
  --downlink-in "<虚拟扬声器采集>" --downlink-out "<物理耳机>" --downlink-target zh
```

- 优先验证：路由隔离拒绝启动、双链路独立状态事件、真实回环暂停、VAD saved_ratio、热插拔后错误提示和设备刷新。

## 7. r2 修复说明

### 7.1 修复范围

- 修复 `crates/engine/src/link.rs`：`run_link` 在 Session 上行发送失败或下行接收关闭后，不再退出 task，而是进入外层重连循环。
- 已打开的 input/output 物理流、VAD 与上下行 resampler 保持在重连循环外，Session 断开后只重建 Gemini Session。
- 上行发送从 `send().await` 改为 `try_send`，避免 channel 满时阻塞整个 `select!`；满队列时本地 pending 队列复用 `gemini_live::session::drop_stale_frames` 语义，只保留最新帧。

### 7.2 行为说明

- 初始连接失败：状态置 `Error(...)` 后退出。
- 已运行链路断开：状态置 `Reconnecting { attempt: 1 }`，重新调用 `connect_with_retry` 建立新 Session。
- 重连阶段继续失败：`attempt` 递增；达到 5 次仍失败后状态置 `Error(...)` 并退出。
- 重连成功：状态回到 `Running`，继续复用原物理流与 resampler 泵音频。
- `LinkHandle.abort()` 仍通过 tokio task abort 中止；外层循环没有忙等。

### 7.3 新增自动测试

- `engine::link::tests::reconnect_budget_retries_then_returns_to_running_budget`：覆盖断开后 `Reconnecting{1}`、连接失败递增到上限、成功后重置。
- `engine::link::tests::upstream_backpressure_keeps_latest_pending_frame`：覆盖上行满队列时不 await、不堆积旧帧，只保留最新 pending frame。

说明：`run_link` 完整音频泵依赖实时音频 ring 与 tokio 调度，直接单测容易受调度时序影响；本轮将重连预算/状态推进抽成确定性单元测试覆盖，真实 WebSocket 断线恢复仍建议 QA 在真机联调项中验证。

### 7.4 r2 自测结果

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `cargo test --workspace`：53 passed；逐 crate：`audio-core` 6、`audio-cpal` 2、`audio-dsp` 8、`cli` 2、`device-manager` 5、`diagnostics` 6、`engine` 13、`gemini-live` 10 unit + 1 integration。
- `cargo run -p cli`：通过；无参打印输入/输出设备表头与参数提示，未 panic。

### 7.5 提交计划

- commit message：`fix: 修复链路断开后的真实重连`
- 文件：`crates/engine/src/link.rs`、`TASK/delivery/DELIVERY-003.md`

## 8. hotfix：下行延迟上限 + 回环检测接线

### 8.1 修复范围

- `crates/gemini-live/src/session.rs`：下行接收任务不再使用 `audio_out.send(frame).await` 阻塞 WebSocket 读取；改为小容量队列 + 非阻塞 `try_send` + 最新帧 pending，队列满时不堆积旧下行音频。
- `crates/audio-cpal/src/lib.rs`：输出环形缓冲从 `rate * channels`（约 1 秒）降为 200ms；输入缓冲保留 1 秒并注明原因。
- `crates/engine/src/link.rs`：下行播放新增 120ms engine 侧 pending 上限，超限丢弃最旧样本；新增低频诊断日志，打印延迟代理、下行丢帧计数和回环证据。
- `crates/engine/src/link.rs` / `crates/engine/src/orchestrator.rs`：将 control event sender 接入 link，运行时调用 `diagnostics::frame_energy`、`detect_loop`、`step_guard`；疑似回环时置 `SessionState::Paused`，发送 `LoopSuspected` 和 `TranslationPaused`，暂停期间停止上行发送和下行注入，清白后发送 `TranslationResumed`。
- `crates/engine/Cargo.toml`：新增 `tracing` 依赖用于 hotfix 诊断日志。

### 8.2 关键设计选择

- 下行 session 队列容量：从 64 帧降为 4 帧，另有 1 帧 latest pending；避免 WS read 被播放速度拖慢。
- CPAL 输出缓冲：200ms，约等于 `rate * channels / 5`，兼顾回调抖动与拖尾上限。
- engine 下行附加缓冲：120ms；超过后从 pending 头部丢旧样本，播放尽量追到最新译文。
- 回环阈值：`min_xcorr=0.85`、`energy_ratio_db=-12dB`、`max_lag_frames=40`、`hold_frames=40`、`release_frames=80`，比 diagnostics 默认值更保守，降低正常对话误暂停概率。
- 真机声学效果未自动验证；本轮只自动覆盖纯逻辑和接线状态推进。

### 8.3 新增自动测试

- `gemini_live::session::tests::lossy_output_send_keeps_latest_pending_frame`：覆盖下行满队列时不阻塞，pending 只保留最新帧。
- `audio_cpal::tests::output_ring_capacity_is_around_two_hundred_ms`：覆盖输出 ring 容量计算。
- `engine::link::tests::downstream_pending_drops_oldest_to_bound_extra_latency`：覆盖下行 pending 超限丢最旧。
- `engine::link::tests::loop_guard_runtime_pauses_and_resumes_with_events`：覆盖回环 guard 接线的暂停/恢复推进。

### 8.4 hotfix 自测结果

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `cargo test --workspace`：57 passed；逐 crate：`audio-core` 6、`audio-cpal` 3、`audio-dsp` 8、`cli` 2、`device-manager` 5、`diagnostics` 6、`engine` 15、`gemini-live` 11 unit + 1 integration。
- 已局部通过：`cargo test -p engine`（15 passed）、`cargo test -p gemini-live`（11 unit + 1 integration passed）、`cargo test -p audio-cpal`（3 passed）。
- `cargo run -p cli`：通过；无参打印输入/输出设备表头与参数提示，未 panic。
- 占位符扫描：目标代码与本次交付/复测文档无未实现或待办占位标记。

### 8.5 QA 复测观察点

- 启动时打开 `RUST_LOG=engine=info,gemini_live=warn`，观察 `链路低频诊断`：`latency_proxy_ms` 应保持在小常量范围，`downstream_dropped_samples/downstream_dropped_ms` 在网络或播放端跟不上时递增。
- 出现回环时应看到 warn 日志 `检测到疑似声学/应用层回环，自动暂停翻译链路`，包含 `lag_frames`、`xcorr`、`ratio_db`；CLI 同时打印 `LoopSuspected`、`TranslationPaused { AcousticLoop }`。
- 暂停期间应停止继续注入译文；回环消失并满足 release 滞回后，CLI 应打印 `TranslationResumed`，链路状态回到 `Running`。

### 8.6 提交计划

- commit message：`fix: 限制下行延迟并接入回环暂停`
- 文件：`crates/gemini-live/src/session.rs`、`crates/audio-cpal/src/lib.rs`、`crates/engine/Cargo.toml`、`crates/engine/src/link.rs`、`crates/engine/src/orchestrator.rs`、`TASK/delivery/DELIVERY-003.md`、`docs/superpowers/notes/M2-validation.md`
