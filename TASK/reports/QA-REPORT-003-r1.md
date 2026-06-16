# 测试报告 QA-REPORT-003-r1

## 1. 概述

- 阶段：M2（双向链路 + 设备管理 + 防循环 + VAD）
- 轮次：r1
- 被测交付文档：`TASK/delivery/DELIVERY-003.md`
- 权威依据：`TASK/TASK-QA-003.md`、`docs/superpowers/plans/2026-06-16-m2-bidirectional-devices.md`（Task 14-26）
- 工具链：rustc/cargo 1.83.0（`~/.cargo/bin`，与 `rust-toolchain.toml` 一致）
- 结论：**打回**（发现 1 个 P2：`engine::link::run_link` 失败后不真正重连，仅单次报告 `Reconnecting` 即终止任务；另发现 1 个 P3 观察项）

## 2. 命令执行结果

```text
$ cargo fmt --all -- --check
EXIT_CODE=0   # 无输出，格式全部符合

$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.22s
EXIT_CODE=0   # 无任何 warning

$ cargo test --workspace
audio-core:      6 passed
audio-cpal:      2 passed
audio-dsp:       8 passed
cli:             2 passed
device-manager:  5 passed
diagnostics:     6 passed
engine:          11 passed
gemini-live:     10 passed (unit)
gemini-live:     1 passed (tests/session_mock.rs 集成测试，真实执行，非跳过)
doc-tests:       0（全部空）
合计：51 passed; 0 failed; 0 ignored
EXIT_CODE=0
```

与 DELIVERY-003 §3 自报数字（51 passed，逐 crate 分布完全一致）独立复测吻合，未发现编造。

```text
$ cargo build --workspace
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.11s
EXIT_CODE=0

$ cargo run -p cli   （无参）
== 输入设备 ==
  [physical] [default] 外置麦克风
== 输出设备 ==
  [physical] [default] 外置耳机

请用 --uplink-in/--uplink-out/--downlink-in/--downlink-out 指定设备后重跑。
EXIT_CODE=0，未 panic
```

说明：本 QA 机器 CPAL 枚举可返回真实物理设备（与 Dev 机器"枚举为空"的沙箱现象不同），恰好验证了 `[physical]`/`[default]` 标签真实命中、表头打印正常、无参不崩溃。因环境无 BlackHole/VB-CABLE，`[virtual-mic]`/`[virtual-speaker]` 标签未能现场观测到命中，但已通过代码核对确认 `print_device` 的 `classify()` 分支逻辑完整（`crates/cli/src/main.rs:64-68`），命中率验证列入待人工项。

**关键发现：`gemini-live` mock WS 集成测试在本环境实际执行通过，并非被跳过。** `crates/gemini-live/tests/session_mock.rs` 中 `TcpListener::bind("127.0.0.1:0")` 在本沙箱未被拒绝（未触发 `PermissionDenied` 分支），完整走了"起本地 WS 服务器 → client connect → 收发 setup/音频帧"闭环并通过全部断言。Dev 交付文档中提及的"沙箱禁止 loopback bind 时跳过"分支在本次 QA 验证中未被触发，覆盖是完整的。

## 3. 用例结果

| # | 用例 | 期望 | 实际 | 结论 |
|---|---|---|---|---|
| 1 | `audio-core` watch 句柄转发 | 初始空，发送后可收到 | `backend.rs::watch_handle_relays_raw_event` 真实建 `mpsc::channel`，先断言 `try_recv().is_none()`，再 `send(ListChanged{..})` 后断言 `matches!(Some(ListChanged{..}))`。断言真实覆盖 | 通过 |
| 2 | `device-manager` 分类 | VB-CABLE Output→VirtualMic；BlackHole Input→VirtualSpeaker；物理麦→Physical | `classify.rs::classify_uses_direction_plus_virtual_name` 三组真实设备名 + 方向组合，逐一 `assert_eq!`。`classify()` 实现与 `(is_virtual, dir)` 真值表完全对应计划 | 通过 |
| 3 | 快照 diff | `[A,B]→[B,C]` → `[Added{C},Removed{A}]`，B 无事件 | `snapshot.rs::diff_added_and_removed_set` 排序后 `assert_eq!`，断言集合恰为两元素，未额外断言 B；代码读取确认 diff 算法对未变化设备不产生事件 | 通过 |
| 4 | 默认变更 | 集合不变、默认迁移 → 仅 `DefaultChanged` | `snapshot.rs::diff_default_change_only` 断言结果恰为单元素 Vec | 通过 |
| 5 | DeviceLost | 在用设备缺失→`DeviceLost`；仍在→空 | `watch.rs::project_lost` + `snapshot.rs::project_lost_only_for_in_use`：MicX 缺失产生 DeviceLost，SpkY 仍在产生空 Vec，两路径都断言 | 通过 |
| 6 | 不可变刷新 | `refresh()` 新快照 `!=` 旧；diff 出语义 Added 且 classify==VirtualSpeaker | `manager.rs::refresh_returns_new_immutable_snapshot`：mock backend 改变后 `assert_ne!(old, new)`，再用 `diff_snapshots` 断言含 `Added{Input,..}`，再 `assert_eq!(classify(..)，VirtualSpeaker)`。全程无 cpal 依赖（`device-manager/Cargo.toml` 仅 audio-core+thiserror） | 通过 |
| 7 | 能量摘要 | 全零 rms=0/peak=0；满幅 peak=MAX/rms 接近满刻度；空切片 n=0 不 panic | `meter.rs::frame_energy_is_zero_alloc_and_correct` 三段全覆盖：`[0;480]`→`FrameEnergy{0,0,480}`；`[i16::MAX;480]`→`peak==MAX && rms>32000`；`frame_energy(&[])`→`n==0`。函数体逐样本累加、无 `Vec::new`/`vec!`/`.to_vec()` | 通过 |
| 8 | 隔离·源汇重叠 | 同设备源汇→`Err`；合法双链路→Ok | `isolation.rs::isolation_rejects_same_device_as_source_and_sink` 覆盖两分支并都断言；合法双链路（PhysMic→VirtMic, VirtSpk→PhysHeadset）断言 `Ok(())` | 通过 |
| 9 | 隔离·输出回流虚拟采集 | 汇是另一链路虚拟采集源→`Err(OutputIsVirtualCaptureSource)`；物理耳机作汇→Ok | `isolation.rs::isolation_rejects_output_to_virtual_capture_source` 精确匹配该枚举变体；对照组在用例8覆盖 | 通过 |
| 10 | 回环命中 | captured=injected 右移12帧-3dB→suspected/lag∈[11,13]/ratio≈-3dB | `loopcheck.rs::detect_loop_flags_delayed_echo`：手工构造右移12帧×0.708衰减序列，三项断言（suspected/lag范围/ratio误差<1.5dB）均执行。`detect_loop` 实现为真实的滑动 lag 扫描求归一化跨相关峰值 + 对齐后 RMS² 比值，非占位 | 通过 |
| 11 | 回环不误判 | ratio<-6dB 或 xcorr<0.6 → suspected=false | `detect_loop_ignores_quiet_or_uncorrelated` 两组场景（极低能量 / 高能不相关）均断言 `!suspected` | 通过 |
| 12 | 滞回 Pause/Resume | 连 hold 帧→Pause；单帧 Clear 不恢复；连 release 帧→Resume；抖动不翻转 | `guard_hysteresis_pause_and_resume`：hold=3/release=3，3 帧疑似后断言唯一 Pause；1 帧 Clear 断言无 action；再 2 帧 Clear 后断言追加 Resume。完整覆盖滞回边界 | 通过 |
| 13 | VAD 静音 | `observe([0;480])==Drop`；100帧全零 `frames_sent==0`/`saved_ratio==1.0` | `vad.rs::silence_frame_is_dropped` 两层断言（单帧 + 100帧统计）都执行，`VadStats::record/saved_ratio` 真实参与计算 | 通过 |
| 14 | VAD attack | 大音量连 attack 帧后 Send/`is_speaking()==true` | `loud_speech_frame_is_sent_after_attack`：先跑 `attack_frames` 次 warmup，再断言第 `attack_frames+1` 次 `Send` 且 `is_speaking()` | 通过 |
| 15 | VAD 幅度滞回 | 边界 RMS：静音态 Drop；speaking 态 Send | `hysteresis_prevents_flapping_on_borderline_rms`：tone(450) 介于 close(300)/open(600) 之间，从静音态进入断言 `Drop`，从 speaking 态进入断言 `Send`，两分支都验证 | 通过 |
| 16 | VAD hangover | 语音后静音：连 hangover 帧仍 Send，之后 Drop 且 `is_speaking()==false` | `hangover_keeps_sending_tail_after_speech_stops` 循环 `hangover_frames` 次断言每帧 `Send`，再断言下一帧 `Drop` 且 `!is_speaking()` | 通过 |
| 17 | RMS 纯函数 | 全零0；直流满幅(误差≤1)；空帧0不panic；同输入一致 | `frame_energy_rms_is_pure_and_correct` 覆盖前三项；"多次一致"未见显式重复调用断言，但函数无内部状态，纯函数性质由签名保证 | 通过（多次一致性为推论非显式断言，记为 P3 观察） |
| 18 | 高 ZCR 低能噪声 | zcr 高、`classify_frame(..,false)==false`、`observe==Drop` | `high_zcr_low_energy_classified_as_noise_not_speech` 三项全部显式断言：`zero_crossing_rate(noise) > zcr_noise_max`、`!classify_frame(..)`、`vad.observe(..)==Drop` | 通过 |
| 19 | `audio-cpal` watch 句柄不崩 | 返回句柄、`try_recv` 不阻塞不panic | `watch_devices_returns_handle_without_panicking` 真实调用 `CpalBackend::watch_devices()` 并 `try_recv()` | 通过 |
| 20 | Paused 投影 | `worst_state(Paused,Reconnecting)==Paused`；`worst_state(Error,Paused)==Error` | `control.rs::worst_picks_paused_over_reconnecting` / `worst_picks_error_over_paused` 两测试精确对应；rank 表 `Error=5,Paused=4,Reconnecting=3,Starting=2,Running=1,Idle=0` 与计划一致 | 通过 |
| 21 | 三态装配 | UplinkOnly→downlink=None；Bidirectional→两链路+downlink.source==Auto+四设备互异 | `route.rs::build_routes_uplink_only_drops_downlink` + `build_routes_bidirectional_lights_two_links`：后者显式断言 `down.source==SourceLang::Auto` 与四组 `assert_ne!` 设备互异 | 通过 |
| 22 | 隔离拒绝 | 上行注入==下行采集→`Err`；合法→Ok | `validate_isolation_rejects_source_sink_overlap` + `validate_isolation_rejects_output_to_virtual_capture` 双场景，且对照合法路由断言 `Ok(())` | 通过 |
| 23 | 缺设备干净失败 | 引用不存在设备→`Err(DeviceNotFound)` 不 panic | `build_routes_missing_device_errs` 断言 `matches!(..,Err(RouteError::DeviceNotFound(_)))` | 通过 |
| 24 | 编排器投影 | `top_state` 复用 `worst_state`；独立上报 UplinkState(Running)/DownlinkState(Reconnecting{2}) | `orchestrator.rs::orchestrator_projects_worst_state_per_link`：用内存 watch 通道构造两条 LinkHandle（绕过真实音频/网络），断言 `top_state()` 等于直接调用 `worst_state(..)` 的结果，再断言 relay 出的两条独立事件均被收到 | 通过 |
| 25 | CLI 参数解析 | `parse_mode` 三态+默认双向；`build_spec` 正确映射 | `main.rs::parse_mode_defaults_and_variants` + `build_spec_maps_args_to_intents` 均真实解析字符串参数后字段级断言 | 通过 |
| 26 | 分类设备表 | 无参运行打印 `[default]/[virtual-mic]/[virtual-speaker]/[physical]` | 实测（见 §2）：本机打印 `[physical] [default]`；virtual-mic/virtual-speaker 标签因无虚拟设备未现场命中，代码核对确认逻辑完整。**列为部分通过 + 待人工**（虚拟设备命中率） | 部分通过（结构验证通过；命中率需人工，已记录于 M2-validation.md） |

## 4. Bug 清单

### BUG-003-01（P2）：链路失败后不会真正重连，"Reconnecting" 状态为终态假象

- **级别**：P2（一般：明显质量问题，违反架构纪律明确条款）
- **标题**：`engine::link::run_link` 在 Session 发送/接收失败时只上报一次 `SessionState::Reconnecting{attempt:1}` 便 `break` 退出 pump 循环，链路 task 随即结束；orchestrator 也未监听链路任务结束并重新拉起。结果是链路永久停在 `Reconnecting` 状态，再也不会变化，与状态名字面含义（"正在重连"）相悖，且违反 `TASK-QA-003.md` 架构纪律核查第4条"第二条下行链路 = 独立 Session/独立 task/独立重连/独立过期帧丢弃"以及计划文件 `Architecture` 段"独立重连"的要求。
- **复现步骤**（代码可静态复现，无需真机）：
  1. 阅读 `crates/engine/src/link.rs:111-138` 的 `run_link` 主循环。
  2. 上行分支（118-122行）：`audio_tx.send(frame16).await` 失败时，`state_tx.send(Reconnecting{1})` 后 `break`，循环结束，函数返回，`tokio::spawn` 的外层 task 退出。
  3. 下行分支（124-128行）：`audio_rx.recv()` 返回 `None` 时同样 `Reconnecting{1}` 后 `break`，task 结束。
  4. 全文件搜索确认：无任何地方对失败后的 task 重新调用 `connect_with_retry` 或重新 `spawn_link`；`orchestrator.rs` 的 `spawn_state_relay`（66-101行）只是单纯转发 watch 变化为事件，并不监控 task 是否存活、也不重启。
- **期望 vs 实际**：
  - 期望（按架构纪律 + 计划"独立重连"描述）：链路应在网络/Session 异常后自动重试连接（如重新走 `connect_with_retry`），状态在 `Reconnecting{attempt}` 与 `Running` 之间正常转移，`attempt` 应随重试次数递增；只有重试上限耗尽才应转为 `Error`。
  - 实际：链路只把 `attempt` 硬编码为 `1` 上报一次，随后线程/task 彻底终止，不会再发任何状态。UI/CLI 收到的最后状态永久停留在 `Reconnecting{attempt:1}`，与"链路已死、不会恢复"的真实情况不符，且没有任何机制能让它恢复成 `Running`。
- **影响**：直接影响 M2 验收标准里"两链路独立断线重连，一条临时断网另一条仍 Running 自动重连"这一核心卖点——当前实现下，**任一链路一旦断一次网就永久死亡**，不存在"自动重连"这件事，只是状态机字面上声称在重连。这会让 Task 26 待人工项 #2（两链路独立断线重连）在真机测试时必然失败，不是"环境限制不可测"，而是代码层面就没有实现重连循环。
- **定位**：`crates/engine/src/link.rs:111-138`（`run_link` 主循环），对照 `crates/engine/src/orchestrator.rs:44-101`（`start`/`spawn_state_relay` 均无重启逻辑）。

### 观察项-003-02（P3）：`gemini-live::drop_stale_frames` 未在新版 `link.rs` 下行泵中接线

- **级别**：P3（轻微，不阻断）
- **标题**：M1 阶段实现并单测的 `drop_stale_frames`（`crates/gemini-live/src/session.rs:42`）在 M2 新的 `engine::link::run_link` 下行处理路径（`crates/engine/src/link.rs:124-136`）中未被调用；下行队列依赖 `tokio::sync::mpsc::channel(64)` 的固定容量与发送端 backpressure 间接限流，未显式做"过期帧丢弃"。
- **复现**：`grep -rn "drop_stale_frames" crates/engine/src/` 无匹配。
- **期望 vs 实际**：架构纪律核查第4条提到"独立过期帧丢弃"；现状是有界 channel 提供了背压但没有真正按"新鲜度"丢弃过期帧的语义（先进先出排队，不丢旧帧只是等待）。
- **定位**：`crates/engine/src/link.rs:124-136`；对照 `crates/gemini-live/src/session.rs:42-47`。
- **结论**：因计划 Task 25 对 `link.rs` 下行段描述未强制要求调用该函数（只写"Session.recv → 切块 resample → out_rate → push_slice"），且有界 channel 在弱负载下行为可接受，本项定为 P3，不阻断本轮，建议下一阶段补上。

## 5. 架构/依赖纪律核查

| 项 | 结果 |
|---|---|
| `engine`/`device-manager`/`diagnostics`/`cli` 源码无 `#[cfg(target_os)]` | 通过（`grep -rn 'cfg(target_os' crates/{engine,device-manager,diagnostics,cli}/src` 空） |
| `device-manager`/`diagnostics`/`engine` 的 `Cargo.toml` 不依赖 `cpal`/`audio-cpal` | 通过（三份 Cargo.toml 逐一核对，均无 cpal/audio-cpal 依赖；唯一引入处是 `cli`/`audio-cpal` 本身） |
| 数据面零分配：`frame_energy`/VAD 按 `&[i16]` 借用、无堆分配 | 通过（`meter.rs::frame_energy`、`vad.rs` 全部核心函数体内无 `Vec::new`/`vec!`/`.to_vec()`；`vad.rs` 唯一 `Vec` 出现在测试辅助函数 `tone()`） |
| `cargo build --workspace` 闭合 Task14 trait 缺口 | 通过，`audio-cpal::watch_devices` 已实现（轮询线程+空列表保护，逻辑与计划一致） |
| `GEMINI_API_KEY` 走环境变量、无硬编码密钥 | 通过（`crates/cli/src/main.rs:32` 用 `std::env::var`；全仓 grep 未见硬编码 key） |
| 交付物无 `unimplemented!`/`TODO`/`FIXME` | **通过，且为本轮重点核实项**：`grep -rn "unimplemented!\|TODO\|FIXME" crates/ --include="*.rs"` 全仓零匹配。重点复核了计划中原本标记 `unimplemented!` 的两处：`diagnostics::loopcheck::detect_loop`（已替换为真实滑动 lag 扫描 + 归一化跨相关 + 对齐 RMS² 比值实现，三项测试全部通过断言细节）与 `engine::link::spawn_link`（已替换为真实 `open_input/open_output → connect_with_retry → tokio::select! 双向泵` 实现，见 BUG-003-01 对其重连质量的进一步审查） |
| `worst_state` 复用未重写 | 通过（`orchestrator.rs::top_state` 直接调用 `crate::control::worst_state`，未重新实现 rank 逻辑） |
| downlink 走"听者定目标"（source==Auto、target 锁听者语言） | 通过（`route.rs::build_routes` 第96-101行downlink分支硬编码 `source: SourceLang::Auto`，测试 `build_routes_bidirectional_lights_two_links` 显式断言） |

## 6. 待人工验证项状态（Task 26）

`docs/superpowers/notes/M2-validation.md` 已就位，为待填空白模板（日期/平台/虚拟设备/各模式结果/双向30分钟/防循环/VAD/防双声/热插拔/差距 共9个待填条目），符合"可为待填模板"的要求。以下项目按 `TASK-QA-003.md` 逐条标注状态（均为**需人工**，不计入自动 Bug 门槛）：

| 项 | 状态 |
|---|---|
| 真机中英双向30分钟 | 需人工（无真实 GEMINI_API_KEY + 会议软件环境） |
| 两链路独立断线重连 | 需人工验证；**但代码审查已发现该功能在当前实现下必然失败（见 BUG-003-01）**，人工测试预期会复现此问题而非环境限制 |
| 三态省成本（仅上行/仅下行对应方向未建连接） | 需人工（可观测网络连接数，本沙箱无法连接真实 wss://） |
| 防循环第二道防线真实声学环路触发 | 需人工（无真实声卡环路） |
| 防双声主观听感 | 需人工（主观听感，无法自动化） |
| VAD 真机阈值标定 | 需人工（真实语音环境） |
| 热插拔 DeviceLost→Error 子态 | 需人工（无可插拔虚拟/物理设备且 CLI 当前未将 `watch_devices`/`project_lost` 接入运行时循环，见下方补充说明） |
| 虚拟设备真实命名命中率 | 需人工（无 BlackHole/VB-CABLE 设备） |
| M2-validation.md 模板就位 | **已确认就位**，结构完整，9 项待填 |

补充说明（非 Bug，记录供下一阶段参考）：当前 `cli`/`engine` 运行时主循环未调用 `device_manager::DeviceManager::refresh()`/`watch_devices()` 做热插拔轮询——`DeviceManager` 仅在启动时打印一次性设备表。这与计划 Task 22-26 的范围一致（计划未要求把热插拔监听接入运行时编排循环），故不计入 Bug，但意味着"热插拔时序"这条人工验收项在当前代码下即使有真实设备也无法触发 `DeviceLost`/`Error` 转换，需在后续阶段补线。

## 7. 结论

- 三连命令（fmt/clippy/test）全部独立复测通过，测试总数 51，与交付文档一致，未发现数字编造。
- `gemini-live` mock WS 集成测试在本环境**实际运行通过**（非沙箱跳过路径），覆盖完整。
- 架构纪律（无 `cfg(target_os)`、device-manager/diagnostics/engine 不依赖 cpal、数据面零分配、无硬编码密钥、`worst_state` 复用、downlink source=Auto）全部核查通过。
- **占位符扫描全仓零命中**，且重点复核确认 `detect_loop`/`spawn_link` 均为真实实现，非遗留 `unimplemented!`。
- 但代码审查发现 **BUG-003-01（P2）**：链路失败后不会真正重连，仅上报一次 `Reconnecting` 状态后任务永久终止，违反架构纪律"独立重连"条款，且会导致 Task 26 待人工项"两链路独立断线重连"在真机测试时必然失败。
- 另记录 1 个 P3 观察项（`drop_stale_frames` 未接线），不阻断。

**存在 P2 → 结论：打回。**

**打回 Dev 必修项**：
1. 修复 `engine::link::run_link`（`crates/engine/src/link.rs:111-138`），使链路在 Session 发送/接收失败后真正发起重连（复用 `connect_with_retry` 或等价机制），状态在 `Reconnecting{attempt}` 递增与 `Running` 之间正常往返，仅重试耗尽才转 `Error`；并补充一条可自动验证的单测（如用可控的 mock session 工厂模拟一次失败后恢复，断言状态序列包含 `Reconnecting→Running`，而非停在 `Reconnecting` 不动）。

修复后请进入 r2 轮，QA 将重点复测该项与回归全量三连。
