# 设计文档：跨平台实时双向语音翻译应用 — 端到端架构与里程碑路线

> 基于 `prd.md`，本文档聚焦"如何落地"：端到端技术架构 + 可执行的里程碑路线。
> 日期：2026-06-13

---

## 0. 关键前提（已核实）

**Gemini 3.5 Live Translate 真实存在**（2026-06 正式发布），是本架构的基石能力：

- **端到端语音到语音模型**：音频进、翻译后音频出，无需自己拼 STT→翻译→TTS；保留说话人语气/语速/音高。
- 通过 **Gemini Live API**（WebSocket 实时流）以 public preview 开放，支持 70+ 语言。
- **源语言自动识别**，并在 `inputTranscription` 返回检测到的语言码。
- **目标语言必须显式指定**（`targetLanguageCode`），**单 Session 内目标固定、不能中途切换、不支持单 Session 双向**。
- `echoTargetLanguage` 选项：当输入已是目标语言时可"原样重复"或"静音"。
- 现实约束：为保证上下文准确，模型**刻意保持几秒延迟**。→ PRD 9.1 的激进延迟指标需在 M1 用真实数据重新校准。
- Rust 侧大概率**无官方 SDK**，需基于 WebSocket 协议自实现客户端（M1 的最大单点风险）。

来源：Google blog / Gemini API docs (Live Translate) / MarkTechPost，2026-06。

---

## 1. 定调决策

| 决策项 | 选择 | 理由 |
|---|---|---|
| 讨论焦点 | 端到端架构 + 里程碑路线 | — |
| 技术栈策略 | **Rust/Tauri 从头搭，全程复用**（非"原型后重写"） | 代码一路复用到 V1.0 |
| 平台策略 | **双平台并行**，从 V0.1 定义平台无关 `AudioBackend` trait | 架构更干净；成本：需双机联调 + 双平台 CI |
| 管线并发模型 | **A 方案：线程化环形缓冲管线** | 隔离硬实时音频与软实时网络，最防爆音/jitter |
| auto 模式语义 | **解读一：听者定目标** | 满足"用户无需预声明语言"，且不扰动实时核心 |

---

## 2. 管线并发模型（A 方案）

- 音频采集/播放跑在**由系统音频回调驱动的专用实时线程**：回调内**零分配、零 `await`、零锁**。
- 各阶段之间用**无锁环形缓冲区**（如 `ringbuf`）传递 PCM。
- 只有 Gemini WebSocket 网络 I/O 跑在独立 **Tokio 运行时**，通过环形缓冲与实时线程解耦。
- 吸收 Actor 模型的"清晰边界"思想：**数据面走环形缓冲，控制面走消息/channel**。

**线程/缓冲拓扑（双向模式）：**

```
[物理麦克风回调线程] --ring--> [上行处理: VAD/降噪/重采样] --ring--> [Tokio: 上行Gemini Session]
                                                                          |
   [虚拟麦克风输出回调线程] <--ring-- [上行播放缓冲] <----------------------+

[虚拟扬声器采集回调线程] --ring--> [下行处理: VAD/降噪/重采样] --ring--> [Tokio: 下行Gemini Session]
                                                                          |
   [物理耳机输出回调线程] <--ring-- [下行播放缓冲] <-----------------------+
```

上下行是**两条完全独立的链路、两个独立 Gemini Session**；单向模式只起其中一条。

---

## 3. 模块与 Crate 架构

Cargo workspace 多 crate，固化 PRD 逻辑分层：

```
translate/  (Cargo workspace)
├── crates/
│   ├── audio-core/        # 平台无关：AudioBackend trait、PcmFrame、环形缓冲封装
│   ├── audio-wasapi/      # Windows 平台特有扩展位（热插拔/独占模式等 CPAL 覆盖不到的细节）
│   ├── audio-coreaudio/   # macOS 平台特有扩展位
│   ├── audio-dsp/         # 重采样(rubato)、VAD、降噪/AGC/AEC(webrtc-audio-processing)
│   ├── gemini-live/       # Gemini Live WebSocket 客户端：连接/收发/重连/错误分类
│   ├── engine/            # 编排核心：会话生命周期、路由、控制面状态机
│   ├── device-manager/    # 设备枚举、默认标记、热插拔、虚拟设备探测
│   └── diagnostics/       # 延迟/音量/连接状态采集 + 日志
├── src-tauri/             # Tauri 后端：把 engine 暴露给前端 (command/event)
└── ui/                    # React/Vue 前端
```

**关键接口边界（各 crate 只暴露窄接口）：**

- `audio-core::AudioBackend` — 列举/打开输入输出流；回调把 PCM 推入/拉出环形缓冲。WASAPI/CoreAudio 各实现一遍，上层不感知平台。
- `gemini-live::Session` — `send_audio(frame)` / `recv_audio() -> stream`；内部管 WebSocket、重连、过期帧丢弃。对上层只暴露"喂 16kHz、收 24kHz"。
- `engine` — 唯一"大脑"：持有所有环形缓冲两端，跑控制面状态机，接 UI 指令、发状态事件。Tauri 层只是薄桥。

**MVP 音频底座**：两平台统一用 **CPAL**（内部已封装 WASAPI/CoreAudio）；`audio-wasapi`/`audio-coreaudio` 作为平台特有扩展位，遇到 CPAL 天花板再下沉原生 API。

**双平台纪律**：所有平台差异收敛在 `audio-wasapi`/`audio-coreaudio` 内；`engine` 及以上**禁止 `#[cfg(target_os)]`**。CI 双平台跑，任一平台编译失败即红。

---

## 4. auto 模式（解读一：听者定目标）

核心洞察：**目标语言由"谁在听"决定，而非"谁在说"。**

- 上行 Session（听者=B）→ 目标固定 = B 的语言；A 说什么由模型自动识别源语言。
- 下行 Session（听者=A）→ 目标固定 = A 的语言。
- 同语言情形交给 `echoTargetLanguage = silent`。

**对控制面的影响：最小且受控。**

- 会话配置加开关：`source: Locked(lang) | Auto`。auto = 建 Session 时不锁源、只设 `targetLanguageCode = 听者语言`。
- 两个 Session 全程目标语言不变，**实时数据通路一行不改，无需中途重配**。
- 模型返回的检测语言码 → 控制面状态通道**新增只读事件** `DetectedLanguage{stream, code}`，UI 显示"检测到对方说英文"，并为将来字幕预埋数据。

（解读二"完全动态语言对"作为 M4 可选增强：新增 `Reconfiguring` 状态 + 语言映射表，需销毁重建 Session，有冷启动问题。）

---

## 5. 全双工路由 + 防回声/防循环

### 5.1 路由矩阵（启动时生成，运行中只读）

| 模式 | 上行链路 | 下行链路 |
|---|---|---|
| 双向 | 物理麦→上行Session→**虚拟麦克风** | 虚拟扬声器采集→下行Session→**物理耳机** |
| 仅上行 | 物理麦→上行Session→虚拟麦克风 | 关闭（下行 Session 不启动，省成本） |
| 仅下行 | 关闭 | 虚拟扬声器采集→下行Session→物理耳机 |

每条链路源/汇设备固定且不重叠 — 架构层面杜绝循环的第一道闸。

### 5.2 防音频循环（三道防线）

1. **物理隔离**：上行注入"虚拟麦克风"、下行采集"虚拟扬声器"，二者不同设备，链路本身不构成环。
2. **回环检测**：`diagnostics` 持续比对"上行注入信号"与"输入端采集信号"的能量+延迟相关性；判定循环 → **自动暂停翻译** + 弹配置修复向导。
3. **AEC 兜底**：`audio-dsp` 启用 WebRTC AEC，喂入扬声器参考信号消物理串音（兼治不用耳机的回声）。

### 5.3 防双声

MVP 默认"只听译文"：下行只把**译文**送物理耳机，原始远端音频**不直接进物理输出**（只作为下行 Session 输入）。原文监听为 M3+ 可选项。

### 5.4 半双工约束

每 Session 目标固定 + 几秒延迟 → 上下行各一条独立 Session 是**必须**。两 Session 在 Tokio 中为独立 task，各自独立重连/背压。

---

## 6. 双平台抽象 + 错误/重连状态机

### 6.1 `AudioBackend` trait

```rust
trait AudioBackend {
    fn list_inputs(&self) -> Vec<DeviceInfo>;      // 含 is_default、is_virtual
    fn list_outputs(&self) -> Vec<DeviceInfo>;
    fn open_input(&self, id: &DeviceId, cfg: StreamCfg) -> Result<InputStream>;
    fn open_output(&self, id: &DeviceId, cfg: StreamCfg) -> Result<OutputStream>;
    fn watch_devices(&self) -> Receiver<DeviceEvent>; // 热插拔
}
```

- 流回调只做一件事：推入/拉出环形缓冲（零分配/零 await/零锁）。
- 虚拟设备识别靠名字匹配：Windows = `VB-Audio`/`CABLE`，macOS = `BlackHole`。
- 采样率差异由 `rubato` 重采样兜底（设备 48k ↔ Gemini 16k 进/24k 出）。

### 6.2 错误/重连状态机

```
        ┌─────┐  start   ┌──────────┐  all-ready  ┌─────────┐
        │ Idle│ ───────► │ Starting │ ──────────► │ Running │
        └─────┘          └──────────┘             └────┬────┘
           ▲                   │ fail                  │ ws drop / net loss
           │ stop              ▼                       ▼
           │              ┌──────────┐  retry≤N   ┌──────────────┐
           └──────────────┤  Error   │◄───────────┤ Reconnecting │
                          └──────────┘  giveup    └──────┬───────┘
                                                          │ recovered
                                                          └────► Running
```

- 上行/下行各有**独立**子状态：一条 Reconnecting，另一条继续 Running，不全停；整体取"最坏"投影给 UI。
- `gemini-live` 负责：指数退避重连；断线时 `send` 队列**丢弃过期帧**（防延迟堆积）；重连后 `recv` 对齐。
- 错误**分类**透出：`AuthError` / `NetworkError` / `ServerError` / `TokenExpired` — UI 据此给不同文案与操作。
- 设备热插拔 → `DeviceLost` 事件 → 进 Error 子态提示重选，不崩溃。

### 6.3 控制面 vs 数据面边界（贯穿全设计）

- **控制面**：低频、消息驱动（channel）、可 async — 启停/设备选择/模式切换/状态/错误/检测语言。住在 `engine` + Tauri command/event。
- **数据面**：高频、实时、环形缓冲 — PCM 在回调线程与 Tokio 网络线程间流动，永不被控制面阻塞。
- 唯一交汇：控制面经**原子标志/无锁命令**通知数据面（静音/暂停/切缓冲）；数据面绝不反向阻塞等待。

---

## 7. 里程碑路线（沿 crate 边界逐层点亮，非"原型后重写"）

### M0 · 地基（约 1 周）
- 建 workspace 与全部空 crate 骨架；定义 `AudioBackend` trait、`PcmFrame`、环形缓冲、`engine` 控制面消息枚举。
- 双平台 CI 立起来（macOS + Windows 编译 + 单测）。
- **验收**：两平台 `cargo test` 绿；接口签名定型。

### M1 · 验证 Gemini 链路（对应 PRD V0.1，约 1–2 周）
- 实现 `gemini-live` Rust WebSocket 客户端（连接/发16k/收24k/重连/错误分类）— **最大单点风险，优先打掉**。
- `audio-core` + CPAL：物理麦采集 → rubato 重采样 → Gemini → 播放物理扬声器；单向、命令行、双平台。
- 接 auto 模式 + `echoTargetLanguage`，验证延迟。
- **验收**：中英任一方向实时翻译，连续 10 分钟，记录 P50/P95 延迟（校准 PRD 9.1）。

### M2 · 双向 + 虚拟设备 MVP（对应 PRD V0.2，约 2–3 周）
- 点亮第二条链路（虚拟扬声器采集→下行 Session→物理耳机），两 Session 独立运行。
- `device-manager`：枚举/默认标记/虚拟设备探测（BlackHole / VB-CABLE）/热插拔。
- 路由矩阵 + 防循环（先上物理隔离 + 回环检测自动暂停，AEC 可滞后）。
- VAD 省成本。
- **验收**：Zoom/Teams 真机中英双向，连续 30 分钟不崩、无明显循环。

### M3 · 桌面 Beta（对应 PRD V0.3，约 3–4 周）
- Tauri + React/Vue：主界面状态、配置向导、系统托盘、延迟/音量电平、错误提示、日志导出。
- `diagnostics` 全量接 UI；`DetectedLanguage` 上屏。
- 第三方软件配置引导（Zoom/Teams/Discord/微信/企业微信）。
- AEC 兜底、原文监听可选项（如有余力）。
- **验收**：非技术用户照向导完成配置；连续 60 分钟；断线重连可见可恢复。

### M4 · 产品化（对应 PRD V1.0，独立大里程碑，不阻塞前面）
- 自有虚拟音频驱动（macOS AudioServerPlugin / Windows APO/driver）— **风险最高，单列里程碑**，前三里程碑全程用 BlackHole/VB-CABLE 不依赖它。
- 账号体系 + 服务端临时 Token（API Key 不进客户端）、额度控制、自动更新、崩溃上报。
- 可选增强：auto 解读二动态语言对；字幕（复用 M1 起返回的 transcript）。

**关键路径与并行性**：M0→M1 严格串行（地基 + Gemini 客户端是一切前提）；M1 之后音频/设备工作与 Tauri/UI 工作在 M2/M3 可并行。两大风险点 — Rust Gemini 客户端（M1）、自有虚拟驱动（M4）— 已被前置或隔离。

---

## 8. 未决 / 待 M1 验证

- Gemini Live 真实端到端延迟（决定 PRD 9.1 指标是否需放宽）。
- Rust 侧是否有可用社区 crate，还是全自研 WebSocket 客户端。
- CPAL 在两平台对虚拟设备枚举/热插拔的覆盖度（决定何时下沉原生 API）。
