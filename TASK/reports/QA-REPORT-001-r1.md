# 测试报告 QA-REPORT-001-r1

## 1. 概述

- **阶段：** M0 地基
- **轮次：** r1（第 1 轮）
- **被测交付文档：** `TASK/delivery/DELIVERY-001.md`
- **结论：** 通过

---

## 2. 命令执行结果

### `cargo fmt --all -- --check`

```
（无输出，退出码 0）
```

结果：通过。

---

### `cargo clippy --workspace --all-targets -- -D warnings`

```
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.07s
```

结果：通过，无警告。

---

### `cargo test --workspace`

```
running 5 tests
test frame::tests::le_bytes_roundtrip ... ok
test backend::tests::detects_virtual_devices_by_name ... ok
test frame::tests::duration_of_16k_frame ... ok
test ring::tests::push_then_pop_roundtrip ... ok
test ring::tests::push_drops_overflow_without_blocking ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 3 tests
test control::tests::both_running_is_running ... ok
test control::tests::worst_picks_reconnecting_over_running ... ok
test control::tests::worst_picks_error_over_running ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

Doc-tests audio_core: 0 passed
Doc-tests engine: 0 passed
```

结果：8 passed，0 failed，退出码 0。

---

## 3. 用例结果

| # | 用例 | 期望 | 实际 | 结论 |
|---|---|---|---|---|
| 1 | workspace 编译 | `cargo build --workspace` 成功，含 audio-core / engine | `Finished dev profile`，两 crate 均存在并通过编译 | 通过 |
| 2 | PcmFrame 时长 | 1600 样本 @16k → 100ms | `duration_of_16k_frame` 断言 `abs(duration_ms - 100.0) < 1e-9`，测试通过 | 通过 |
| 3 | PcmFrame 字节序 | `to_le_bytes`/`from_le_bytes` 往返一致，4 样本 → 8 字节 | `le_bytes_roundtrip` 断言 `bytes.len()==8` 且还原帧与原帧相等，测试通过 | 通过 |
| 4 | 环形缓冲往返 | push [1,2,3] 后 pop 得 [1,2,3] | `push_then_pop_roundtrip` 断言 push 返回 3、pop 返回 3 且内容一致，测试通过 | 通过 |
| 5 | 环形缓冲满 | 容量 4 推 6 个 → 返回 4，不阻塞 | `push_drops_overflow_without_blocking` 断言返回 4，pop 验证得 `[1,2,3,4]`，测试通过 | 通过 |
| 6 | 虚拟设备识别 | BlackHole / VB-Audio CABLE → true；普通麦克风/Realtek → false | `detects_virtual_devices_by_name` 四个断言全部通过（"BlackHole 2ch" true, "CABLE Output (VB-Audio Virtual Cable)" true, "MacBook Pro Microphone" false, "Realtek High Definition Audio" false） | 通过 |
| 7 | 最坏态投影 | Error > Reconnecting > Running；双 Running → Running | 三个测试 `worst_picks_error_over_running` / `worst_picks_reconnecting_over_running` / `both_running_is_running` 全部通过，rank 函数赋值 Error=4 > Reconnecting=3 > Starting=2 > Running=1 > Idle=0，逻辑正确 | 通过 |
| 8 | CI 配置 | `.github/workflows/ci.yml` 含 windows-latest + macos-latest，跑 fmt+clippy+test | 文件存在，matrix 含 `[windows-latest, macos-latest]`，steps 依次执行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` | 通过 |

---

## 4. 架构纪律核查

| 检查项 | 结果 |
|---|---|
| `crates/engine` 源码无 `#[cfg(target_os)]` | 通过。`grep -r "#[cfg(target_os" crates/engine/` 无命中 |
| `crates/audio-core` 源码无 `#[cfg(target_os)]` 实际属性 | 通过。仅在 `backend.rs` 文档注释中出现字符串"#[cfg(target_os)]"作为说明，无实际属性 |
| `audio-core` 仅依赖平台无关库（无 cpal/平台 crate） | 通过。`crates/audio-core/Cargo.toml` 仅声明 `thiserror` 和 `ringbuf`，无 cpal 或任何平台相关依赖 |
| 环形缓冲满时不阻塞 | 通过。实现直接委托 `ringbuf::Producer::push_slice`，返回实际写入数，无阻塞路径，测试在 0.00s 完成 |

---

## 5. 语义差异备注（非 Bug，已知差异）

DELIVERY-001 §4 明确说明：计划文档（Task 3 注释）原文写"缓冲满时丢弃最旧样本（丢旧）"，但 `ringbuf 0.4.8` 普通 `Producer::push_slice` 的实际语义为"只写入可容纳的前缀，溢出的**新**样本被丢弃（丢新）"。

**判断：**
- TASK-QA-001 用例5 的验收期望仅为"返回 4，不阻塞"，未要求保留最新数据。当前实现满足该期望。
- `ring.rs` 文档注释与实际语义已修正一致（注释说"溢出的新样本被丢弃"）。
- 测试断言（pop 得到 `[1,2,3,4]`）正确反映了"丢新"语义，测试与实现自洽。
- 此差异在技术上属于"丢新不丢旧"策略，对实时音频场景意味着：当缓冲满时，最新采集的音频帧被丢弃，保留了旧帧。这与常见"实时宁可丢旧"最佳实践相反，但在当前 M0 阶段属于可接受的已知约束，后续 M1+ 如有需要可切换为 `push_slice_overwrite`。
- 此差异**不构成 P1/P2 Bug**，DELIVERY-001 已如实披露，测试与实现自洽，验收标准字面达标。记录为 P3 观察项。

---

## 6. Bug 清单

| 编号 | 级别 | 标题 | 复现 | 期望 vs 实际 | 定位 |
|---|---|---|---|---|---|
| BUG-001 | P3 | 环形缓冲语义为"丢新"而非计划注释说的"丢旧" | 创建容量4缓冲，先 push [1,2,3,4]（填满），再 push [5,6]（溢出）；pop 后得 [1,2,3,4] | **期望**（按计划注释"丢旧"语义）：得 [3,4,5,6]；**实际**：得 [1,2,3,4]，新样本被丢弃 | `crates/audio-core/src/ring.rs`：整体实现；`docs/superpowers/plans/2026-06-13-m0-foundation-m1-gemini-link.md` Task 3 注释第1行 |

> BUG-001 说明：该差异已在 DELIVERY-001 §4 主动披露，ring.rs 内注释已与实现同步修正。TASK-QA-001 用例5验收标准（"返回4，不阻塞"）字面达标。定级 P3（可改进项，不阻断进入下一阶段），供后续设计决策参考。

---

## 7. 结论

- **P0：** 0 条
- **P1：** 0 条
- **P2：** 0 条
- **P3：** 1 条（BUG-001，环形缓冲"丢新"与计划注释语义不一致，已知差异，测试自洽，不阻断）

**结论：通过**。仅有 P3 级别观察项，无 P0/P1/P2，可进入 M1 阶段。

- 轮次：r1（第 1 轮即通过）
- 人工介入：不需要
