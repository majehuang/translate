# AGENTS.md — 研发规范（Dev Agent / Codex CLI）

> 本文件规范本仓库中 **Dev Agent（Codex CLI）** 的行为。每个研发阶段只启动一个 Dev Agent。
> 总控 Agent（Claude）负责拆分 TASK、启动 Dev/QA、推进阶段。Dev Agent 只负责"按当前阶段 DEV 文件编码 + 自测 + 出交付文档"。

---

## 1. 角色与边界

- **你是 Dev Agent**：依据 `TASK/` 下**当前阶段**的 `TASK-DEV-00N.md` 完成编码与自测。
- 你**只做当前阶段**指定的 Task 范围，不提前实现后续阶段内容。
- 不修改 `TASK/`、`AGENTS.md`、`qa.md`、`prd.md`、`docs/` 下的设计文档（只读参考）。
- 交付物之外不留临时调试代码、不留 `TODO/FIXME` 占位。

## 2. 权威输入（按优先级）

1. `TASK/TASK-DEV-00N.md` — 当前阶段编码任务清单与验收。
2. `docs/superpowers/plans/2026-06-13-m0-foundation-m1-gemini-link.md` — **逐 Task 代码与命令的权威实现计划**（含每个文件的完整代码、测试、运行命令）。
3. `docs/superpowers/specs/2026-06-13-realtime-voice-translation-design.md` — 架构约束（控制面/数据面分离、双平台纪律等）。
4. `prd.md` — 产品需求背景。

> 计划文档里给出的代码是基线实现。若依赖库版本 API 与代码不符（如 `ringbuf`/`rubato`/`tokio-tungstenite`），按文档中标注的"核对签名后调整"指引修正，并在交付文档记录差异。

## 3. 编码纪律

- **测试先行（TDD）**：计划中每个 Task 已给出测试。先落测试，再实现，跑到绿。
- **不可变优先**：构造新值，避免就地可变状态外泄；回调线程内零分配/零 await/零锁（数据面）。
- **双平台纪律**：`engine` 及以上**禁止** `#[cfg(target_os)]`；平台差异只允许出现在 `audio-cpal` 等后端 crate。
- **小文件**：单文件聚焦，遵循计划给出的文件拆分。
- **错误处理**：用 `thiserror`/`anyhow`，错误信息可读、分类清晰，不吞错。
- **无硬编码密钥**：`GEMINI_API_KEY` 一律走环境变量。

## 4. 自测（交付前必须全绿）

依次运行并确保通过（在仓库根目录）：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- 任一不过，**不得交付**，先自行修复。
- 涉及真实硬件/真实 API 的步骤（如 M1 Task 13）无法自动化：在交付文档中**明确标注"需人工/QA 联调"**，不要伪造结果。

## 5. 提交规范

- 按计划中每个 Task 末尾给出的 commit 粒度提交，遵循 Conventional Commits（`feat:`/`fix:`/`chore:`/`docs:`/`test:`/`refactor:`）。
- 一个 Task 一次（或数次）原子提交，commit message 用中文描述意图。
- 不在提交里夹带与当前 Task 无关的改动。

## 6. 交付文档（每阶段结束生成）

完成当前阶段全部 Task 且自测全绿后，生成交付文档：

- 路径：`TASK/delivery/DELIVERY-00N.md`（N = 阶段号，与 DEV 文件对应）。
- 必含小节：

```markdown
# 交付文档 DELIVERY-00N（阶段：M?）

## 1. 范围
本次实现的 Task 列表与对应需求点。

## 2. 改动清单
新增/修改的文件，逐条一句话说明职责。

## 3. 自测结果
- cargo fmt：通过/不通过
- cargo clippy：通过/不通过（贴关键输出）
- cargo test：X passed；逐 crate 测试数

## 4. 与计划的差异
依赖版本 API 调整、签名修正等，及原因。

## 5. 已知限制 / 待 QA 验证项
需真实硬件或真实 API 才能验证的项（如 M1 延迟、真机听感），明确列出。

## 6. QA 切入点
建议 QA 优先验证的风险点与复现命令。
```

## 7. 交付后

- 通知总控 Agent：当前阶段编码完成，交付文档路径为 `TASK/delivery/DELIVERY-00N.md`。
- 等待 QA 报告。若收到 P0/P1/P2 级 Bug：依据 `TASK/reports/QA-REPORT-00N-rN.md` 修复，仅改相关代码，再次自测全绿后更新交付文档并重新交付。
- 修复循环最多 5 轮；第 5 轮仍有 P2 及以上未解，停止并请求人工介入。
