# M2 真机验证操作指南（Task 26）

> 配合 `M2-validation.md`（记录表）使用：本文件讲**怎么做**，把结果填进 `M2-validation.md`。
> M2 自动验收（53 测试）已通过；本指南覆盖无法自动化、需真实虚拟声卡 + 会议软件 + 真实 `GEMINI_API_KEY` 的部分。

---

## 0. 前置：装两个虚拟声卡（关键）

防循环第一道防线要求**上行注入的"虚拟麦克风"与下行采集的"虚拟扬声器"是两个不同设备**，故需两个：

```bash
brew install blackhole-2ch blackhole-16ch
```

装完注销重登。确认分类表出现两个 `[virtual-*]`：

```bash
export PATH="$HOME/.cargo/bin:$PATH"; cd /Users/maje/Workspace/translate
cargo run -p cli
```

期望（标签来自 CLI `classify`）：
```
[virtual-mic]       BlackHole 2ch        # 输出方向虚拟设备 → 注入译文
[virtual-speaker]   BlackHole 16ch       # 输入方向虚拟设备 → 采集对方原声
[physical][default] 外置麦克风
[physical][default] 外置耳机
```

---

## 1. 路由拓扑（中文用户 ↔ 英文对方）

```
你说中文 → [外置麦克风] → CLI 上行 Session(中→英) → [BlackHole 2ch]  → Zoom 麦克风（对方听英文）
对方说英文 → Zoom 扬声器 → [BlackHole 16ch] → CLI 下行 Session(英→中) → [外置耳机]（你听中文）
```

- 上行听者 = 对方 → `--uplink-target en`
- 下行听者 = 你 → `--downlink-target zh`
- 上述正好是 CLI 默认值。

---

## 2. 会议软件配置（Zoom / Teams）

- **麦克风** = `BlackHole 2ch`（接收 CLI 注入的译文）
- **扬声器** = `BlackHole 16ch`（CLI 从这里采集对方原声）
- 关闭"自动调整麦克风音量"与降噪，避免干扰。

---

## 3. 启动 CLI（双向）

```bash
export PATH="$HOME/.cargo/bin:$PATH"; cd /Users/maje/Workspace/translate
GEMINI_API_KEY='你的key' cargo run --release -p cli -- \
  --mode bidirectional \
  --uplink-in "外置麦克风"        --uplink-out "BlackHole 2ch" \
  --downlink-in "BlackHole 16ch"  --downlink-out "外置耳机" \
  --uplink-target en --downlink-target zh
```

看到 `Running` / 顶层状态后开始测。日志打印 `UplinkState` / `DownlinkState` / 防循环等 `ControlEvent`。

---

## 4. 逐项验收（对照 `M2-validation.md`）

| 验收项 | 操作 | 期望 |
|---|---|---|
| 物理隔离拒绝启动 | 把 `--uplink-out` 与 `--downlink-in` 设成**同一设备** | CLI 拒绝启动并报隔离错误，不进 Running（无需 Zoom） |
| 设备分类 | 第 0 步分类表 | 两虚拟设备标签正确 |
| 三态省成本 | 分别 `--mode uplink-only` / `downlink-only` | 仅对应方向进 Running，另一方向不建 Session（可 `nettop` 看仅一条 wss） |
| 双向听感 | 真人对说 | 你说中文对方听英文；对方说英文你听中文 |
| 防双声 | 听你的外置耳机 | 只听到中文译文，不直接听到对方英文原声 |
| 30 分钟稳定性 | 连续双向 30 分钟 | 记录崩溃次数 / 循环回声 / 延迟堆积 |
| 两链路独立重连（r2 修复项） | 运行中临时断网 3–5 秒再恢复 | 某链路 `Reconnecting{attempt:N}` 后自行回 `Running`，另一链路保持 Running |
| 防循环自动暂停 | 制造声学环路（让麦克风采到自身输出） | 数秒内 `LoopSuspected`/`TranslationPaused`，自动暂停 |
| 热插拔 | 运行中拔掉外置麦克风 | `DeviceLost` → 对应链路 Error 子态、不崩溃；插回可恢复 |
| VAD 省成本 | 静默 vs 说话 | 静默不持续发上行帧；说话及时恢复，句首不被吃 |

---

## 5. 建议顺序（从易到难）

1. **无需 Zoom**：分类 → 隔离拒绝启动 → `uplink-only` 单向跑通（复用 M1 经验），确认接线没坏。
2. **加 Zoom**：双向听感 + 防双声。
3. **压力项**：断网重连 → 热插拔 → 30 分钟 → 防循环。

---

## 6. 注意事项

- 两个虚拟设备别接反：`--uplink-out` = Zoom 的**麦克风**设备；`--downlink-in` = Zoom 的**扬声器**设备。接反会被隔离校验拒绝或没声音。
- 鉴权 `?key=`、模型 `models/gemini-3.5-live-translate-preview`（已固化进代码）。
- 走本地代理偶发 `tls handshake eof`，由内置指数退避重连兜底。
- 联调结束请到 AI Studio **轮换 `GEMINI_API_KEY`**。
