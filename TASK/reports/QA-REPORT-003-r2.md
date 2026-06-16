# 测试报告 QA-REPORT-003-r2

> 注：本轮 QA Sonnet 子代理在执行中触发会话 token 上限中断，未能写出报告。由总控 Agent 直接完成 r2 复验（独立运行三连 + 代码/测试审计）并记录于此。

## 1. 概述
- 阶段：M2（双向链路 + device-manager + diagnostics 防循环 + VAD）
- 轮次：r2（复验 r1 打回项）
- 被测交付：`TASK/delivery/DELIVERY-003.md`（含 r2 修复说明）+ commit `bd7a841`
- 结论：**通过**，可进入下一阶段（真机项除外，见 §5）。

## 2. 命令执行结果（总控独立复跑）
- `cargo fmt --all -- --check`：通过，无差异。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过，0 警告。
- `cargo test --workspace`：**53 passed**，0 failed（engine 13，含 r2 新增 2 条重连/背压单测；其余 crate 与 r1 一致）。
- `cargo run -p cli`（无参）：打印分类设备表，未 panic。
- `grep -rn "unimplemented!|TODO|FIXME" crates/`：零命中。

## 3. r1 遗留项复验

### BUG-001（P2）链路断开后假重连 → **已修复**
- `crates/engine/src/link.rs` `run_link` 现为【外层重连循环】：
  - `loop` 内 `connector(cfg).await` 真正重建 Session；失败经 `ReconnectBudget::connect_failed` → `Retry(Reconnecting{attempt递增})` 则 `continue` 重连，达上限 `GiveUp(Error)` 才 `return`（`link.rs:122-159`）。
  - 物理 input/output 流、VAD、上/下行 resampler 在循环外创建、跨重连复用（`link.rs:111-119`）。
  - `pump_session` 返回（断开）后 `state_tx.send(reconnect.disconnected())` 置 `Reconnecting{attempt:1}` 再回环。
  - `abort()` 仍可干净停止（task 级 AbortHandle）。
- 回归测试真实覆盖：`reconnect_budget_retries_then_returns_to_running_budget` 断言 disconnected→Reconnecting{1}、connect_failed→Retry{2}/{3}、再失败→GiveUp(Error)、connected() 后预算复位。非空断言。

### BUG-002（P3）drop_stale_frames 未接线 → **已处理**
- 上行改为 `try_send`，不再 `send().await` 阻塞 `select!`；满队列时本地 pending 经 `drop_stale_frames(pending, UPSTREAM_PENDING_KEEP)` 只保留最新帧（`try_flush_upstream`/`enqueue_latest_upstream`）。
- 回归测试 `upstream_backpressure_keeps_latest_pending_frame`：channel(1) 满时保留最新帧（样本 3），验证丢旧留新。

## 4. 回归核查
- 设备分类 / 快照 diff / DeviceLost、能量摘要、物理隔离、回环检测+滞回、VAD、路由三态、编排器 worst_state 投影、CLI 纯函数：r1 已通过项测试数不变，全绿。
- 架构/依赖纪律：engine/device-manager/diagnostics/cli 无 `#[cfg(target_os)]`；device-manager/diagnostics 不依赖 cpal；数据面零分配/锁/await；无硬编码密钥。
- gemini-live mock 集成测试在本机实际运行通过（非沙箱跳过路径）。

## 5. 待人工验证项（Task 26，不计入门槛）
`docs/superpowers/notes/M2-validation.md` 模板就位。需真机（BlackHole/VB-CABLE + Zoom/Teams + 真实 GEMINI_API_KEY）：双向 30 分钟稳定性、两链路独立断线重连（代码层已修，待真机确认）、三态省成本、防循环真实触发、防双声主观、VAD 真机阈值、热插拔时序、虚拟设备命名命中率。

## 6. 结论
- P0/P1/P2：**0**。P3：0（r1 的 P3 已处理）。
- 轮次：r2，**通过**。M2 自动验收达标，进入真机联调阶段。
