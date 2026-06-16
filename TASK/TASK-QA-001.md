# TASK-QA-001 · 阶段 M0 地基（测试）

> QA Agent（Claude Sonnet 子代理）执行。依据 `TASK/delivery/DELIVERY-001.md` 与本文件验证 M0 交付。规范见 `qa.md`。

## 验收门槛
无 P0/P1/P2 即通过，可进入 M1。

## 必跑命令（贴真实输出）
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## 用例清单

| # | 用例 | 期望 |
|---|---|---|
| 1 | workspace 编译 | `cargo build --workspace` 成功，含 audio-core / engine |
| 2 | PcmFrame 时长 | 1600 样本 @16k → 100ms |
| 3 | PcmFrame 字节序 | `to_le_bytes`/`from_le_bytes` 往返一致，4 样本 → 8 字节 |
| 4 | 环形缓冲往返 | push [1,2,3] 后 pop 得 [1,2,3] |
| 5 | 环形缓冲满丢旧 | 容量 4 推 6 个 → 返回 4，不阻塞 |
| 6 | 虚拟设备识别 | BlackHole / VB-Audio CABLE → true；普通麦克风/Realtek → false |
| 7 | 最坏态投影 | Error > Reconnecting > Running；双 Running → Running |
| 8 | CI 配置 | `.github/workflows/ci.yml` 含 windows-latest + macos-latest，跑 fmt+clippy+test |

## 架构纪律核查
- [ ] `crates/engine` 源码无 `#[cfg(target_os)]`。
- [ ] 环形缓冲满时是丢弃溢出而非阻塞（看 `push_slice` 实现/测试）。
- [ ] `audio-core` 仅依赖平台无关库（无 cpal/平台 crate）。

## 报告
生成 `TASK/reports/QA-REPORT-001-r{round}.md`（见 `qa.md §5`）。含 P0/P1/P2 则打回 Dev。
