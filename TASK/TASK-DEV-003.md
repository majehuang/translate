# TASK-DEV-003 · 阶段 M2 双向链路 + 设备管理 + 防循环 + VAD（编码）

> Dev Agent（Codex CLI）执行。权威实现细节见 `docs/superpowers/plans/2026-06-16-m2-bidirectional-devices.md` 的 **Task 14–26**（M2 切片）。本文件给范围与验收，**代码以计划文档为准**。

## 前置
M0/M1 已通过 QA。`gemini-live` 已实测打通真实 API（模型 `models/gemini-3.5-live-translate-preview`、鉴权 `?key=`、`translationConfig` 在 `generationConfig` 内）。

## 目标
在 M1 单向链路基础上点亮第二条独立下行链路，交付 Zoom/Teams 真机中英双向通话能力。新增 `device-manager` 与 `diagnostics` 两个平台无关 crate；在 `engine` 落成路由矩阵（双向/仅上行/仅下行）与双链路编排器；在 `audio-dsp` 落成 VAD 省成本；CLI 升级为驱动 engine 编排器的薄壳。**所有新增逻辑必须平台无关、可单测；平台差异（热插拔轮询）只允许新增在 `audio-cpal`。**

## 工作内容（对应计划 Task 14–26）

- **Task 14 · audio-core trait 破坏性扩展**：`backend.rs` 新增 `watch_devices()->DeviceWatchHandle`、`RawDeviceEvent::ListChanged`、`Direction`。测试 1 个（句柄转发原始事件）。引入 trait 缺口，由 Task 22 补齐。
- **Task 15 · device-manager 分类**：新建 crate（仅依赖 audio-core+thiserror，**严禁 cpal**）；`classify(info, dir)->DeviceUse`（方向+虚拟名）。测试 1 个。
- **Task 16 · device-manager 快照 diff**：`snapshot.rs` `diff_snapshots`、`watch.rs` `project_lost`（DeviceLost 区别于普通 Removed）。测试 3 个。
- **Task 17 · DeviceManager**：`manager.rs` 不可变快照 + refresh/snapshot/default_for/pick_use/resolve；mock backend 验证 refresh 产生新快照、原始事件经纯函数到语义事件全程无 cpal。测试 1 个（含端到端纯函数链）。
- **Task 18 · diagnostics 能量摘要**：新建 crate（仅依赖 audio-core）；`frame_energy(&[i16])->FrameEnergy` 零分配。测试 1 个。
- **Task 19 · diagnostics 物理隔离（第一道防线）**：`isolation.rs` `validate_isolation(&[LinkRoute])`（源汇重叠 / 输出回流虚拟采集源）。测试 2 个。
- **Task 20 · diagnostics 回环检测（第二道防线）**：`loopcheck.rs` `detect_loop`（延迟回声判定）+ `step_guard` 滞回状态机。测试 3 个。`detect_loop` 由测试驱动实现到绿，**交付物不得留 `unimplemented!`**。
- **Task 21 · audio-dsp VAD**：`vad.rs` `frame_energy_rms`/`zero_crossing_rate`/`classify_frame`/`Vad`（attack+hangover 滞回）/`VadStats`。纯整数、零分配。测试 6 个。
- **Task 22 · audio-cpal 热插拔（唯一平台码）**：实现 `watch_devices()` 轮询线程（~1s，复用 list 超时，**空列表保护避免误报全量 Removed**）。测试 1 个（句柄可创建不 panic）。此后 `cargo build --workspace` 闭合 Task 14 缺口。
- **Task 23 · engine 控制面扩展**：`control.rs` 新增 `SessionState::Paused`、`ControlEvent::{LoopSuspected,TranslationPaused,TranslationResumed}`、`PauseReason`；`worst_state` rank 纳入 Paused（介于 Reconnecting 与 Error）。`ControlEvent` 去 `Eq` 留 `PartialEq`（含 f32）。测试 2 个。
- **Task 24 · engine 路由矩阵**：`route.rs` `build_routes`（三态装配，downlink 听者定目标 source=Auto）/`validate_isolation`（委托 diagnostics）/`active_links`；`RouteError` 经 `From<IsolationError>`。engine 新增 gemini-live/audio-dsp/diagnostics/device-manager/tokio/thiserror 依赖，**不引入 audio-cpal**。测试 5 个。
- **Task 25 · engine link + orchestrator**：`link.rs` `LinkHandle`/`spawn_link`（独立 Session/重连/背压）；`orchestrator.rs` 聚合两 watch、worst_state 投影、独立上报 Uplink/DownlinkState。内存 watch/channel 单测覆盖状态投影与两链路独立上报。测试 1 个。`spawn_link` 真实音频/网络接线由真机覆盖；**交付物不得留 `unimplemented!`**（spawn_link 须实现真实接线）。
- **Task 26 · CLI 薄壳 + 真机联调（手动）**：解析 `--mode` 与双链路设备/target 参数；无参打印分类设备表；`build_routes`+`validate_isolation` 装配，隔离失败拒绝启动；`Orchestrator::start` + 转发 ControlEvent；Ctrl+C stop。`parse_mode`/`build_spec` 纯函数单测 2 个。**真机双向 30 分钟 + 防循环触发 + 热插拔 + VAD 省成本 + 防双声需真实 GEMINI_API_KEY + BlackHole/VB-CABLE + Zoom/Teams，标注"需人工/QA 联调"，不得伪造结果**；写 `docs/superpowers/notes/M2-validation.md`。

## 验收标准（M2 检查点）
- [ ] `cargo test --workspace` 全绿（M2 新增约 33：audio-core 1、device-manager 5、diagnostics 6、audio-dsp +6、audio-cpal +1、engine +13、cli 2）。
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 无警告。
- [ ] `cargo fmt --all -- --check` 通过。
- [ ] `cargo build --workspace` 成功；`cargo run -p cli`（无参）打印**分类**设备表（`[default]`/`[virtual-mic]`/`[virtual-speaker]`/`[physical]`）。
- [ ] **架构纪律**：`engine`/`device-manager`/`diagnostics`/`cli` 无 `#[cfg(target_os)]`；平台差异只在 `audio-cpal`（新增的 watch 轮询）。`device-manager`/`diagnostics` 的 Cargo.toml 不依赖 cpal/audio-cpal。
- [ ] 路由三态：`active_links` 与 `build_routes` 验证仅上行时 downlink=None、仅下行时 uplink=None、双向两链路；downlink `source==Auto`（听者定目标）。
- [ ] 物理隔离：源汇重叠 / 输出回流虚拟采集源 → `validate_isolation` 返回 Err。
- [ ] 回环检测：延迟回声命中、安静/不相关不命中；滞回 hold→Pause、release→Resume，抖动不翻转。
- [ ] VAD：静音全 Drop（saved_ratio=1.0）、attack 去抖后 Send、幅度滞回不抖动、hangover 补尾、高 ZCR 低能判噪声。
- [ ] 编排器：`top_state` 复用 `worst_state`；两链路独立上报 UplinkState/DownlinkState（一条重连不拖另一条）。
- [ ] 数据面纪律：能量摘要/VAD 按 `&[i16]` 借用、零堆分配；回调线程无 await/锁/分配。
- [ ] `GEMINI_API_KEY` 走环境变量，无硬编码密钥。
- [ ] 交付物无 `unimplemented!`/`TODO`/`FIXME`；Task 26 真机项明确标注待人工/QA 验证（不阻塞编译/单测验收）。

## 交付
完成后按 `AGENTS.md §6` 生成 `TASK/delivery/DELIVERY-003.md`，把以下列入"待 QA 验证项"：双向 30 分钟稳定性、两链路独立断线重连、三态省成本观测、防循环回环检测触发、防双声主观、VAD 真机阈值标定、热插拔时序、虚拟设备真实命名命中率。