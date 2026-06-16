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
- `cargo test --workspace`：51 passed。

逐 crate 测试数：

- `audio-core`：6 passed。
- `audio-cpal`：2 passed。
- `audio-dsp`：8 passed。
- `cli`：2 passed。
- `device-manager`：5 passed。
- `diagnostics`：6 passed。
- `engine`：11 passed。
- `gemini-live`：10 unit passed + 1 integration passed。

额外验收：

- `cargo build --workspace`：通过。
- `cargo build -p cli`：通过。
- `cargo run -p cli`：通过；当前沙箱环境枚举到的输入/输出设备为空，打印分类设备表头后正常退出，未 panic。
- 架构扫描：`engine` / `device-manager` / `diagnostics` / `cli` 无 `#[cfg(target_os)]`。
- 占位符扫描：`crates/` 下无 `unimplemented!` / `TODO` / `FIXME`。
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
