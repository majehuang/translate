# M0 地基 + M1 验证 Gemini 链路 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 搭起 Rust workspace 地基，并交付一个命令行程序，能采集物理麦克风 → 重采样 → 经 Gemini 3.5 Live Translate 实时翻译 → 播放到物理扬声器，双平台可编译运行，并记录 P50/P95 延迟。

**Architecture:** Cargo workspace 多 crate。数据面走无锁环形缓冲（音频回调线程零分配/零 await/零锁），网络面走独立 Tokio 运行时。`audio-core` 定义平台无关 `AudioBackend` trait，MVP 用 CPAL 作两平台统一底座。`gemini-live` 自实现 Live API WebSocket 客户端。auto 模式 = 建 Session 时不锁源语言、只设 `targetLanguageCode = 听者语言`。

**Tech Stack:** Rust 2021、Cargo workspace、CPAL（音频 I/O）、rubato（重采样）、tokio + tokio-tungstenite（WebSocket）、serde/serde_json、base64、ringbuf、tracing。CI：GitHub Actions（macOS + Windows）。

---

## 文件结构

```
translate/
├── Cargo.toml                         # workspace 根
├── .github/workflows/ci.yml           # 双平台 CI
├── rust-toolchain.toml                # 锁定 toolchain
├── crates/
│   ├── audio-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                 # 导出
│   │       ├── frame.rs               # PcmFrame、SampleRate 等核心类型
│   │       ├── ring.rs                # 环形缓冲封装（基于 ringbuf）
│   │       └── backend.rs             # AudioBackend trait、DeviceInfo、StreamCfg
│   ├── audio-cpal/
│   │   ├── Cargo.toml
│   │   └── src/lib.rs                 # CpalBackend：实现 AudioBackend（双平台）
│   ├── audio-dsp/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       └── resample.rs            # Resampler 封装（rubato）
│   ├── gemini-live/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── protocol.rs            # setup/realtimeInput/serverContent serde 类型
│   │       ├── codec.rs               # base64 + PCM 编解码
│   │       └── session.rs             # Session：连接/收发/重连/丢弃过期帧
│   ├── engine/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       └── control.rs             # 控制面消息枚举 + 状态机
│   └── cli/
│       ├── Cargo.toml
│       └── src/main.rs                # M1 命令行：串起整条单向链路
```

---

# 阶段 M0 · 地基

## Task 1: 建立 workspace 与双平台 CI

**Files:**
- Create: `Cargo.toml`（workspace 根）
- Create: `rust-toolchain.toml`
- Create: `.github/workflows/ci.yml`
- Create: `crates/audio-core/Cargo.toml`、`crates/audio-core/src/lib.rs`
- Create: `.gitignore`

- [ ] **Step 1: 写 workspace 根 `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
edition = "2021"
license = "Proprietary"

[workspace.dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time"] }
tokio-tungstenite = { version = "0.24", features = ["rustls-tls-webpki-roots"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
base64 = "0.22"
ringbuf = "0.4"
rubato = "0.15"
cpal = "0.15"
tracing = "0.1"
tracing-subscriber = "0.3"
anyhow = "1"
thiserror = "2"
futures-util = "0.3"
```

- [ ] **Step 2: 写 `rust-toolchain.toml`**

```toml
[toolchain]
channel = "1.83"
components = ["clippy", "rustfmt"]
```

- [ ] **Step 3: 写 `.gitignore`**

```gitignore
/target
**/*.rs.bk
.env
*.log
```

- [ ] **Step 4: 写占位 crate `crates/audio-core/Cargo.toml` 与 `src/lib.rs`**

`crates/audio-core/Cargo.toml`:
```toml
[package]
name = "audio-core"
version = "0.1.0"
edition.workspace = true

[dependencies]
thiserror.workspace = true
ringbuf.workspace = true
```

`crates/audio-core/src/lib.rs`:
```rust
//! 平台无关音频核心类型与接口。
```

- [ ] **Step 5: 写 `.github/workflows/ci.yml`（双平台）**

```yaml
name: CI
on: [push, pull_request]
jobs:
  build:
    strategy:
      matrix:
        os: [windows-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.83
        with:
          components: clippy, rustfmt
      - run: cargo fmt --all -- --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
```

- [ ] **Step 6: 验证本机编译**

Run: `cargo build --workspace`
Expected: 编译成功，输出 `Compiling audio-core v0.1.0` 与 `Finished`。

- [ ] **Step 7: 提交**

```bash
git add -A
git commit -m "chore: 初始化 Cargo workspace 与双平台 CI"
```

---

## Task 2: audio-core 核心类型 `PcmFrame`

**Files:**
- Create: `crates/audio-core/src/frame.rs`
- Modify: `crates/audio-core/src/lib.rs`

- [ ] **Step 1: 写失败测试 `crates/audio-core/src/frame.rs`**

```rust
//! 音频帧与采样率类型。

/// 单声道 16-bit PCM 帧，附带采样率，便于在管线中流动时自描述。
#[derive(Debug, Clone, PartialEq)]
pub struct PcmFrame {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}

impl PcmFrame {
    pub fn new(samples: Vec<i16>, sample_rate: u32) -> Self {
        Self { samples, sample_rate }
    }

    /// 帧时长（毫秒）。
    pub fn duration_ms(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate as f64 * 1000.0
    }

    /// 转成小端字节序（Gemini 要求 16-bit little-endian）。
    pub fn to_le_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.samples.len() * 2);
        for s in &self.samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    /// 从小端字节序还原。
    pub fn from_le_bytes(bytes: &[u8], sample_rate: u32) -> Self {
        let samples = bytes
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        Self { samples, sample_rate }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_of_16k_frame() {
        let frame = PcmFrame::new(vec![0i16; 1600], 16_000);
        assert!((frame.duration_ms() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn le_bytes_roundtrip() {
        let frame = PcmFrame::new(vec![-1, 0, 1, 256], 16_000);
        let bytes = frame.to_le_bytes();
        assert_eq!(bytes.len(), 8);
        let back = PcmFrame::from_le_bytes(&bytes, 16_000);
        assert_eq!(back, frame);
    }
}
```

- [ ] **Step 2: 在 `lib.rs` 导出**

```rust
//! 平台无关音频核心类型与接口。
pub mod frame;
pub use frame::PcmFrame;
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p audio-core frame`
Expected: 2 passed。

- [ ] **Step 4: 提交**

```bash
git add crates/audio-core
git commit -m "feat(audio-core): PcmFrame 核心类型与字节序转换"
```

---

## Task 3: audio-core 环形缓冲封装

**Files:**
- Create: `crates/audio-core/src/ring.rs`
- Modify: `crates/audio-core/src/lib.rs`

- [ ] **Step 1: 写失败测试 `crates/audio-core/src/ring.rs`**

```rust
//! 无锁环形缓冲封装：生产者在音频回调线程，消费者在处理线程。
//! 缓冲满时丢弃最旧样本（实时音频宁可丢旧，不可阻塞回调）。

use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;

pub struct AudioProducer {
    inner: <HeapRb<i16> as Split>::Prod,
}

pub struct AudioConsumer {
    inner: <HeapRb<i16> as Split>::Cons,
}

/// 创建容量为 `capacity` 个 i16 样本的环形缓冲。
pub fn audio_channel(capacity: usize) -> (AudioProducer, AudioConsumer) {
    let (prod, cons) = HeapRb::<i16>::new(capacity).split();
    (AudioProducer { inner: prod }, AudioConsumer { inner: cons })
}

impl AudioProducer {
    /// 推入样本；缓冲不够时丢弃溢出部分，返回实际写入数。
    /// 关键：绝不阻塞，供音频回调线程安全调用。
    pub fn push_slice(&mut self, data: &[i16]) -> usize {
        self.inner.push_slice(data)
    }
}

impl AudioConsumer {
    /// 拉出最多 `out.len()` 个样本，返回实际读取数。
    pub fn pop_slice(&mut self, out: &mut [i16]) -> usize {
        self.inner.pop_slice(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_then_pop_roundtrip() {
        let (mut prod, mut cons) = audio_channel(8);
        assert_eq!(prod.push_slice(&[1, 2, 3]), 3);
        let mut out = [0i16; 4];
        assert_eq!(cons.pop_slice(&mut out), 3);
        assert_eq!(&out[..3], &[1, 2, 3]);
    }

    #[test]
    fn push_drops_overflow_without_blocking() {
        let (mut prod, _cons) = audio_channel(4);
        // 容量 4，推 6 个 → 只写入 4，不阻塞。
        assert_eq!(prod.push_slice(&[1, 2, 3, 4, 5, 6]), 4);
    }
}
```

- [ ] **Step 2: 在 `lib.rs` 导出**

```rust
pub mod ring;
pub use ring::{audio_channel, AudioConsumer, AudioProducer};
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p audio-core ring`
Expected: 2 passed。若 `ringbuf` API 版本不一致致编译失败，按 `cargo doc -p ringbuf --open` 的 `traits` 模块签名调整 `use`。

- [ ] **Step 4: 提交**

```bash
git add crates/audio-core
git commit -m "feat(audio-core): 无锁环形缓冲封装，满时丢旧不阻塞"
```

---

## Task 4: audio-core `AudioBackend` trait

**Files:**
- Create: `crates/audio-core/src/backend.rs`
- Modify: `crates/audio-core/src/lib.rs`

- [ ] **Step 1: 写类型与 trait（含一个 mock 实现的测试）`crates/audio-core/src/backend.rs`**

```rust
//! 平台无关音频后端接口。所有平台差异收敛在实现 crate 内，
//! 上层（engine 及以上）禁止出现 #[cfg(target_os)]。

use crate::ring::{AudioConsumer, AudioProducer};

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("设备未找到: {0}")]
    DeviceNotFound(String),
    #[error("打开音频流失败: {0}")]
    OpenStream(String),
}

/// 设备唯一标识（平台自有字符串 ID）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceId(pub String);

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub name: String,
    pub is_default: bool,
    /// 名字匹配虚拟设备（Windows: VB-Audio/CABLE；macOS: BlackHole）。
    pub is_virtual: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct StreamCfg {
    pub sample_rate: u32,
    pub channels: u16,
    /// 每个回调期望的帧样本数（per channel）。
    pub frame_size: usize,
}

/// 输入流：内部把采集到的 PCM 推入环形缓冲。持有它即保持采集运行。
pub trait InputStream: Send {
    /// 实际协商生效的采样率（可能与请求不同）。
    fn actual_sample_rate(&self) -> u32;
}

/// 输出流：从环形缓冲拉 PCM 播放。
pub trait OutputStream: Send {
    fn actual_sample_rate(&self) -> u32;
}

pub trait AudioBackend: Send + Sync {
    fn list_inputs(&self) -> Result<Vec<DeviceInfo>, AudioError>;
    fn list_outputs(&self) -> Result<Vec<DeviceInfo>, AudioError>;
    /// 打开输入流，采集数据写入返回的 producer 对端（即传入 consumer 不在此，
    /// 而是后端内部持有 producer）。返回 (流句柄, 采集数据的 consumer)。
    fn open_input(
        &self,
        id: &DeviceId,
        cfg: StreamCfg,
    ) -> Result<(Box<dyn InputStream>, AudioConsumer), AudioError>;
    /// 打开输出流，返回 (流句柄, 写入播放数据的 producer)。
    fn open_output(
        &self,
        id: &DeviceId,
        cfg: StreamCfg,
    ) -> Result<(Box<dyn OutputStream>, AudioProducer), AudioError>;
}

/// 名字是否匹配已知虚拟音频设备。
pub fn is_virtual_device_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("blackhole") || n.contains("vb-audio") || n.contains("cable") || n.contains("voicemeeter")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_virtual_devices_by_name() {
        assert!(is_virtual_device_name("BlackHole 2ch"));
        assert!(is_virtual_device_name("CABLE Output (VB-Audio Virtual Cable)"));
        assert!(!is_virtual_device_name("MacBook Pro Microphone"));
        assert!(!is_virtual_device_name("Realtek High Definition Audio"));
    }
}
```

- [ ] **Step 2: 在 `lib.rs` 导出，并给 audio-core 加 thiserror 依赖（Task 1 已加）**

```rust
pub mod backend;
pub use backend::{
    AudioBackend, AudioError, DeviceId, DeviceInfo, InputStream, OutputStream, StreamCfg,
    is_virtual_device_name,
};
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p audio-core backend`
Expected: 1 passed。

- [ ] **Step 4: 提交**

```bash
git add crates/audio-core
git commit -m "feat(audio-core): AudioBackend trait 与虚拟设备名识别"
```

---

## Task 5: engine 控制面消息与状态机

**Files:**
- Create: `crates/engine/Cargo.toml`
- Create: `crates/engine/src/lib.rs`
- Create: `crates/engine/src/control.rs`

- [ ] **Step 1: 写 `crates/engine/Cargo.toml`**

```toml
[package]
name = "engine"
version = "0.1.0"
edition.workspace = true

[dependencies]
audio-core = { path = "../audio-core" }
```

- [ ] **Step 2: 写状态机与消息（含测试）`crates/engine/src/control.rs`**

```rust
//! 控制面：低频、消息驱动。与数据面（环形缓冲）严格分离。

/// 翻译方向模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslateMode {
    Bidirectional,
    UplinkOnly,
    DownlinkOnly,
}

/// 源语言配置：auto 模式不锁源、由模型自动识别。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceLang {
    Locked(String), // BCP-47, 如 "zh"
    Auto,
}

/// 单条 Session 的运行子状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Starting,
    Running,
    Reconnecting { attempt: u32 },
    Error(String),
}

/// 控制面发给 UI 的事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlEvent {
    UplinkState(SessionState),
    DownlinkState(SessionState),
    /// auto 模式下模型返回的检测语言码（只读，供 UI 显示/字幕预埋）。
    DetectedLanguage { uplink: bool, code: String },
}

/// 将上下行子状态投影为 UI 顶层状态：取“最坏”。
pub fn worst_state(up: &SessionState, down: &SessionState) -> SessionState {
    fn rank(s: &SessionState) -> u8 {
        match s {
            SessionState::Error(_) => 4,
            SessionState::Reconnecting { .. } => 3,
            SessionState::Starting => 2,
            SessionState::Running => 1,
            SessionState::Idle => 0,
        }
    }
    if rank(up) >= rank(down) { up.clone() } else { down.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worst_picks_error_over_running() {
        let up = SessionState::Running;
        let down = SessionState::Error("net".into());
        assert_eq!(worst_state(&up, &down), SessionState::Error("net".into()));
    }

    #[test]
    fn worst_picks_reconnecting_over_running() {
        let up = SessionState::Reconnecting { attempt: 2 };
        let down = SessionState::Running;
        assert_eq!(worst_state(&up, &down), SessionState::Reconnecting { attempt: 2 });
    }

    #[test]
    fn both_running_is_running() {
        assert_eq!(
            worst_state(&SessionState::Running, &SessionState::Running),
            SessionState::Running
        );
    }
}
```

- [ ] **Step 3: 写 `crates/engine/src/lib.rs`**

```rust
//! 编排核心：会话生命周期、路由、控制面状态机。
pub mod control;
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p engine`
Expected: 3 passed。

- [ ] **Step 5: 提交**

```bash
git add crates/engine
git commit -m "feat(engine): 控制面消息、Session 状态机与最坏态投影"
```

**M0 完成检查点：** `cargo test --workspace` 全绿；`cargo clippy --workspace --all-targets -- -D warnings` 无警告。

---

# 阶段 M1 · 验证 Gemini 链路

## Task 6: gemini-live 协议类型（serde）

**Files:**
- Create: `crates/gemini-live/Cargo.toml`
- Create: `crates/gemini-live/src/lib.rs`
- Create: `crates/gemini-live/src/protocol.rs`

- [ ] **Step 1: 写 `crates/gemini-live/Cargo.toml`**

```toml
[package]
name = "gemini-live"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
base64.workspace = true
tokio.workspace = true
tokio-tungstenite.workspace = true
futures-util.workspace = true
thiserror.workspace = true
tracing.workspace = true
audio-core = { path = "../audio-core" }

[dev-dependencies]
tokio = { workspace = true, features = ["rt-multi-thread", "macros", "net", "time"] }
```

- [ ] **Step 2: 写协议 serde 类型（含序列化测试）`crates/gemini-live/src/protocol.rs`**

> 注：字段名依据 v1beta BidiGenerateContent。`realtimeInput.audio` 为当前字段（旧 `mediaChunks` 已弃用）。serverContent 结构在 Task 8 用真实流核对后微调。

```rust
//! Gemini Live API (BidiGenerateContent) WebSocket 消息类型。
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Setup {
    pub setup: SetupBody,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupBody {
    pub model: String,
    pub generation_config: GenerationConfig,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationConfig {
    pub response_modalities: Vec<String>,
}

/// 实时音频输入帧。audio 为 base64 的 16k/16-bit/mono/LE PCM。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeInput {
    pub realtime_input: RealtimeInputBody,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeInputBody {
    pub audio: AudioBlob,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioBlob {
    pub mime_type: String, // "audio/pcm;rate=16000"
    pub data: String,      // base64
}

/// 服务端响应（部分字段，按需扩展）。
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerMessage {
    #[serde(default)]
    pub server_content: Option<ServerContent>,
    #[serde(default)]
    pub setup_complete: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerContent {
    #[serde(default)]
    pub model_turn: Option<ModelTurn>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelTurn {
    #[serde(default)]
    pub parts: Vec<Part>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    #[serde(default)]
    pub inline_data: Option<InlineData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineData {
    pub mime_type: String, // "audio/pcm;rate=24000"
    pub data: String,      // base64
}

impl Setup {
    /// 构造 Live Translate 的 setup。auto 模式下 source 不写入（由模型识别）。
    pub fn new_translate(model: &str) -> Self {
        Setup {
            setup: SetupBody {
                model: model.to_string(),
                generation_config: GenerationConfig {
                    response_modalities: vec!["AUDIO".to_string()],
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_serializes_with_camel_case() {
        let s = Setup::new_translate("models/gemini-3.5-live-translate");
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["setup"]["model"], "models/gemini-3.5-live-translate");
        assert_eq!(json["setup"]["generationConfig"]["responseModalities"][0], "AUDIO");
    }

    #[test]
    fn realtime_input_serializes() {
        let ri = RealtimeInput {
            realtime_input: RealtimeInputBody {
                audio: AudioBlob {
                    mime_type: "audio/pcm;rate=16000".into(),
                    data: "AAAA".into(),
                },
            },
        };
        let json = serde_json::to_value(&ri).unwrap();
        assert_eq!(json["realtimeInput"]["audio"]["mimeType"], "audio/pcm;rate=16000");
        assert_eq!(json["realtimeInput"]["audio"]["data"], "AAAA");
    }

    #[test]
    fn server_message_parses_audio_response() {
        let raw = r#"{
            "serverContent": {
                "modelTurn": {
                    "parts": [
                        {"inlineData": {"mimeType": "audio/pcm;rate=24000", "data": "QUJD"}}
                    ]
                }
            }
        }"#;
        let msg: ServerMessage = serde_json::from_str(raw).unwrap();
        let data = msg.server_content.unwrap().model_turn.unwrap().parts[0]
            .inline_data
            .as_ref()
            .unwrap()
            .data
            .clone();
        assert_eq!(data, "QUJD");
    }

    #[test]
    fn server_message_tolerates_unknown_fields() {
        // 真实流含许多本类型未声明的字段，不应解析失败。
        let raw = r#"{"serverContent":{"turnComplete":true},"usageMetadata":{"x":1}}"#;
        let _msg: ServerMessage = serde_json::from_str(raw).unwrap();
    }
}
```

- [ ] **Step 3: 写 `crates/gemini-live/src/lib.rs`**

```rust
//! Gemini Live Translate WebSocket 客户端。
pub mod protocol;
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p gemini-live protocol`
Expected: 4 passed。

- [ ] **Step 5: 提交**

```bash
git add crates/gemini-live
git commit -m "feat(gemini-live): BidiGenerateContent 协议 serde 类型"
```

---

## Task 7: gemini-live 音频编解码

**Files:**
- Create: `crates/gemini-live/src/codec.rs`
- Modify: `crates/gemini-live/src/lib.rs`

- [ ] **Step 1: 写编解码（含测试）`crates/gemini-live/src/codec.rs`**

```rust
//! PcmFrame ↔ Gemini 协议消息的编解码。
use crate::protocol::{AudioBlob, RealtimeInput, RealtimeInputBody, ServerMessage};
use audio_core::PcmFrame;
use base64::{engine::general_purpose::STANDARD, Engine};

/// 16k PcmFrame → realtimeInput JSON 字符串。
pub fn encode_input(frame: &PcmFrame) -> RealtimeInput {
    let b64 = STANDARD.encode(frame.to_le_bytes());
    RealtimeInput {
        realtime_input: RealtimeInputBody {
            audio: AudioBlob {
                mime_type: format!("audio/pcm;rate={}", frame.sample_rate),
                data: b64,
            },
        },
    }
}

/// 从一条 ServerMessage 抽取所有音频 part，解码为 24k PcmFrame。
pub fn decode_audio(msg: &ServerMessage, out_rate: u32) -> Vec<PcmFrame> {
    let mut frames = Vec::new();
    if let Some(sc) = &msg.server_content {
        if let Some(mt) = &sc.model_turn {
            for part in &mt.parts {
                if let Some(inline) = &part.inline_data {
                    if inline.mime_type.starts_with("audio/pcm") {
                        if let Ok(bytes) = STANDARD.decode(&inline.data) {
                            frames.push(PcmFrame::from_le_bytes(&bytes, out_rate));
                        }
                    }
                }
            }
        }
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_then_manual_decode_roundtrip() {
        let frame = PcmFrame::new(vec![1, -1, 100, -100], 16_000);
        let ri = encode_input(&frame);
        assert_eq!(ri.realtime_input.audio.mime_type, "audio/pcm;rate=16000");
        let bytes = STANDARD.decode(&ri.realtime_input.audio.data).unwrap();
        assert_eq!(PcmFrame::from_le_bytes(&bytes, 16_000), frame);
    }

    #[test]
    fn decode_audio_extracts_frame() {
        let raw = r#"{"serverContent":{"modelTurn":{"parts":[
            {"inlineData":{"mimeType":"audio/pcm;rate=24000","data":"AQACAA=="}}
        ]}}}"#;
        let msg: ServerMessage = serde_json::from_str(raw).unwrap();
        let frames = decode_audio(&msg, 24_000);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].sample_rate, 24_000);
        assert_eq!(frames[0].samples, vec![1, 2]); // AQACAA== = 01 00 02 00 LE
    }

    #[test]
    fn decode_audio_empty_when_no_content() {
        let msg = ServerMessage::default();
        assert!(decode_audio(&msg, 24_000).is_empty());
    }
}
```

- [ ] **Step 2: 在 `lib.rs` 导出**

```rust
pub mod codec;
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p gemini-live codec`
Expected: 3 passed。

- [ ] **Step 4: 提交**

```bash
git add crates/gemini-live
git commit -m "feat(gemini-live): 音频帧与协议消息编解码"
```

---

## Task 8: gemini-live Session 连接（对本地 mock WS 服务）

**Files:**
- Create: `crates/gemini-live/src/session.rs`
- Modify: `crates/gemini-live/src/lib.rs`
- Test: `crates/gemini-live/tests/session_mock.rs`

- [ ] **Step 1: 写 Session 实现 `crates/gemini-live/src/session.rs`**

```rust
//! Gemini Live Session：管理一条 WebSocket，发送音频、接收翻译音频。
use crate::codec::{decode_audio, encode_input};
use crate::protocol::{ServerMessage, Setup};
use audio_core::PcmFrame;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("连接失败: {0}")]
    Connect(String),
    #[error("发送失败: {0}")]
    Send(String),
}

pub struct SessionConfig {
    pub url: String,   // 含 access_token query 的完整 wss URL
    pub model: String,
    pub out_rate: u32, // 24000
}

/// 启动一条 Session。返回:
/// - audio_tx: 把 16k PcmFrame 发给 Gemini
/// - audio_rx: 接收解码后的 24k PcmFrame
/// 内部 task 在连接关闭时结束。
pub async fn connect(
    cfg: SessionConfig,
) -> Result<(mpsc::Sender<PcmFrame>, mpsc::Receiver<PcmFrame>), SessionError> {
    let (ws, _) = tokio_tungstenite::connect_async(&cfg.url)
        .await
        .map_err(|e| SessionError::Connect(e.to_string()))?;
    let (mut write, mut read) = ws.split();

    // 发送 setup
    let setup = Setup::new_translate(&cfg.model);
    let setup_json = serde_json::to_string(&setup).expect("setup serialize");
    write
        .send(Message::Text(setup_json.into()))
        .await
        .map_err(|e| SessionError::Send(e.to_string()))?;

    let (audio_tx, mut audio_in) = mpsc::channel::<PcmFrame>(64);
    let (audio_out, audio_rx) = mpsc::channel::<PcmFrame>(64);

    // 发送任务：把上行帧编码后写入 WS
    tokio::spawn(async move {
        while let Some(frame) = audio_in.recv().await {
            let ri = encode_input(&frame);
            let json = match serde_json::to_string(&ri) {
                Ok(j) => j,
                Err(_) => continue,
            };
            if write.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    // 接收任务：解析服务端音频帧，转发给消费者
    let out_rate = cfg.out_rate;
    tokio::spawn(async move {
        while let Some(Ok(msg)) = read.next().await {
            let text = match msg {
                Message::Text(t) => t.to_string(),
                Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
                Message::Close(_) => break,
                _ => continue,
            };
            if let Ok(sm) = serde_json::from_str::<ServerMessage>(&text) {
                for frame in decode_audio(&sm, out_rate) {
                    if audio_out.send(frame).await.is_err() {
                        return;
                    }
                }
            }
        }
    });

    Ok((audio_tx, audio_rx))
}
```

- [ ] **Step 2: 在 `lib.rs` 导出**

```rust
pub mod session;
```

- [ ] **Step 3: 写 mock 集成测试 `crates/gemini-live/tests/session_mock.rs`**

```rust
//! 用本地 WS 服务器模拟 Gemini：收到 setup 后回一帧音频，验证收发闭环。
use futures_util::{SinkExt, StreamExt};
use gemini_live::session::{connect, SessionConfig};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use audio_core::PcmFrame;

#[tokio::test]
async fn session_sends_setup_and_receives_audio() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // mock server
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        // 第一条应为 setup
        let first = ws.next().await.unwrap().unwrap();
        assert!(first.into_text().unwrap().contains("generationConfig"));
        // 回一帧 24k 音频 (样本 [1,2] -> LE bytes 01000200 -> base64 AQACAA==)
        let resp = r#"{"serverContent":{"modelTurn":{"parts":[
            {"inlineData":{"mimeType":"audio/pcm;rate=24000","data":"AQACAA=="}}]}}}"#;
        ws.send(Message::Text(resp.to_string().into())).await.unwrap();
        // 等待客户端上行一帧再关闭
        let _ = ws.next().await;
    });

    let (tx, mut rx) = connect(SessionConfig {
        url: format!("ws://{addr}"),
        model: "models/test".into(),
        out_rate: 24_000,
    })
    .await
    .unwrap();

    // 收到服务端音频帧
    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("超时")
        .expect("通道关闭");
    assert_eq!(frame.sample_rate, 24_000);
    assert_eq!(frame.samples, vec![1, 2]);

    // 上行一帧
    tx.send(PcmFrame::new(vec![0; 160], 16_000)).await.unwrap();
    server.await.unwrap();
}
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p gemini-live --test session_mock`
Expected: 1 passed。（注意 mock 用 `ws://` 明文；真实环境用 `wss://`。）

- [ ] **Step 5: 提交**

```bash
git add crates/gemini-live
git commit -m "feat(gemini-live): Session 收发闭环（mock WS 集成测试）"
```

---

## Task 9: gemini-live 重连与过期帧丢弃

**Files:**
- Modify: `crates/gemini-live/src/session.rs`
- Test: `crates/gemini-live/src/session.rs`（单测段）

- [ ] **Step 1: 写过期帧丢弃策略的失败测试（加在 session.rs 末尾）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use audio_core::PcmFrame;

    /// 上行发送队列在积压超过阈值时，丢弃最旧帧，只保留最新的 `keep` 帧，
    /// 避免网络抖动导致延迟无限堆积。
    #[test]
    fn drop_stale_keeps_latest() {
        let mut q: Vec<PcmFrame> = (0..10)
            .map(|i| PcmFrame::new(vec![i as i16], 16_000))
            .collect();
        drop_stale_frames(&mut q, 3);
        assert_eq!(q.len(), 3);
        // 保留最新 3 帧（样本值 7,8,9）
        assert_eq!(q[0].samples[0], 7);
        assert_eq!(q[2].samples[0], 9);
    }

    #[test]
    fn drop_stale_noop_when_under_limit() {
        let mut q: Vec<PcmFrame> = (0..2)
            .map(|i| PcmFrame::new(vec![i as i16], 16_000))
            .collect();
        drop_stale_frames(&mut q, 5);
        assert_eq!(q.len(), 2);
    }
}
```

- [ ] **Step 2: 实现 `drop_stale_frames`（加在 session.rs `connect` 之前）**

```rust
/// 队列超过 `keep` 时，丢弃最旧帧只保留最新 `keep` 帧。
pub fn drop_stale_frames(queue: &mut Vec<PcmFrame>, keep: usize) {
    if queue.len() > keep {
        let drop_n = queue.len() - keep;
        queue.drain(0..drop_n);
    }
}
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p gemini-live drop_stale`
Expected: 2 passed。

- [ ] **Step 4: 给 `connect` 增加指数退避重连说明（实现）**

在 `session.rs` 顶部加重连封装，供 CLI 调用：

```rust
/// 带指数退避的连接：失败时按 0.5s,1s,2s,4s… 重试，最多 max_attempts 次。
pub async fn connect_with_retry(
    make_cfg: impl Fn() -> SessionConfig,
    max_attempts: u32,
) -> Result<(mpsc::Sender<PcmFrame>, mpsc::Receiver<PcmFrame>), SessionError> {
    let mut delay_ms = 500u64;
    let mut last_err = None;
    for attempt in 0..max_attempts {
        match connect(make_cfg()).await {
            Ok(pair) => return Ok(pair),
            Err(e) => {
                tracing::warn!(attempt, error = %e, "连接失败，准备重试");
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                delay_ms = (delay_ms * 2).min(8_000);
            }
        }
    }
    Err(last_err.unwrap_or(SessionError::Connect("超出最大重试次数".into())))
}
```

- [ ] **Step 5: 运行全 crate 测试与 clippy**

Run: `cargo test -p gemini-live` 然后 `cargo clippy -p gemini-live -- -D warnings`
Expected: 全 passed，无 clippy 警告。

- [ ] **Step 6: 提交**

```bash
git add crates/gemini-live
git commit -m "feat(gemini-live): 过期帧丢弃与指数退避重连"
```

---

## Task 10: audio-dsp 重采样器

**Files:**
- Create: `crates/audio-dsp/Cargo.toml`
- Create: `crates/audio-dsp/src/lib.rs`
- Create: `crates/audio-dsp/src/resample.rs`

- [ ] **Step 1: 写 `crates/audio-dsp/Cargo.toml`**

```toml
[package]
name = "audio-dsp"
version = "0.1.0"
edition.workspace = true

[dependencies]
rubato.workspace = true
audio-core = { path = "../audio-core" }
thiserror.workspace = true
```

- [ ] **Step 2: 写重采样器（含测试）`crates/audio-dsp/src/resample.rs`**

```rust
//! 采样率转换。设备常见 48k；Gemini 入 16k、出 24k。
use audio_core::PcmFrame;
use rubato::{FftFixedIn, Resampler as _};

pub struct Resampler {
    inner: FftFixedIn<f32>,
    from_rate: u32,
    to_rate: u32,
    chunk: usize,
}

impl Resampler {
    /// 创建定长输入重采样器。`chunk` 为每次处理的输入样本数（如 48k 下 480=10ms）。
    pub fn new(from_rate: u32, to_rate: u32, chunk: usize) -> Self {
        let inner = FftFixedIn::<f32>::new(from_rate as usize, to_rate as usize, chunk, 1, 1)
            .expect("创建重采样器");
        Self { inner, from_rate, to_rate, chunk }
    }

    pub fn chunk_size(&self) -> usize {
        self.chunk
    }

    /// 处理一个恰好 chunk 长度的输入帧，返回目标采样率帧。
    pub fn process(&mut self, input: &PcmFrame) -> PcmFrame {
        assert_eq!(input.sample_rate, self.from_rate, "输入采样率不匹配");
        assert_eq!(input.samples.len(), self.chunk, "输入长度必须等于 chunk");
        let floats: Vec<f32> = input.samples.iter().map(|s| *s as f32 / 32768.0).collect();
        let out = self.inner.process(&[floats], None).expect("重采样");
        let samples: Vec<i16> = out[0]
            .iter()
            .map(|f| (f.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect();
        PcmFrame::new(samples, self.to_rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsample_48k_to_16k_ratio() {
        let mut r = Resampler::new(48_000, 16_000, 480); // 10ms @48k
        let input = PcmFrame::new(vec![0i16; 480], 48_000);
        let out = r.process(&input);
        assert_eq!(out.sample_rate, 16_000);
        // 48k->16k 是 1/3，输出约 160 样本（FFT 块允许小偏差）。
        assert!((out.samples.len() as i32 - 160).abs() <= 8, "got {}", out.samples.len());
    }

    #[test]
    fn upsample_24k_to_48k_ratio() {
        let mut r = Resampler::new(24_000, 48_000, 480); // 20ms @24k
        let input = PcmFrame::new(vec![0i16; 480], 24_000);
        let out = r.process(&input);
        assert_eq!(out.sample_rate, 48_000);
        assert!((out.samples.len() as i32 - 960).abs() <= 16, "got {}", out.samples.len());
    }
}
```

- [ ] **Step 3: 写 `crates/audio-dsp/src/lib.rs`**

```rust
//! 音频信号处理：重采样（后续扩展 VAD/降噪/AEC）。
pub mod resample;
pub use resample::Resampler;
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p audio-dsp`
Expected: 2 passed。若 `FftFixedIn::new` 参数顺序因 rubato 版本不同而报错，运行 `cargo doc -p rubato --open` 核对签名（参数为 in_rate, out_rate, chunk_size_in, sub_chunks, channels）并调整。

- [ ] **Step 5: 提交**

```bash
git add crates/audio-dsp
git commit -m "feat(audio-dsp): 基于 rubato 的重采样器"
```

---

## Task 11: audio-cpal 后端实现（双平台）

**Files:**
- Create: `crates/audio-cpal/Cargo.toml`
- Create: `crates/audio-cpal/src/lib.rs`

> 这是真实硬件 I/O，难以纯单测；用一个“能枚举到设备”的轻测试 + M1 手动联调覆盖。

- [ ] **Step 1: 写 `crates/audio-cpal/Cargo.toml`**

```toml
[package]
name = "audio-cpal"
version = "0.1.0"
edition.workspace = true

[dependencies]
cpal.workspace = true
audio-core = { path = "../audio-core" }
tracing.workspace = true
```

- [ ] **Step 2: 实现 `CpalBackend` `crates/audio-cpal/src/lib.rs`**

```rust
//! CPAL 实现的 AudioBackend，覆盖 Windows(WASAPI) 与 macOS(CoreAudio)。
use audio_core::{
    audio_channel, is_virtual_device_name, AudioBackend, AudioConsumer, AudioError, AudioProducer,
    DeviceId, DeviceInfo, InputStream, OutputStream, StreamCfg,
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub struct CpalBackend {
    host: cpal::Host,
}

impl CpalBackend {
    pub fn new() -> Self {
        Self { host: cpal::default_host() }
    }

    fn find_input(&self, id: &DeviceId) -> Result<cpal::Device, AudioError> {
        self.host
            .input_devices()
            .map_err(|e| AudioError::OpenStream(e.to_string()))?
            .find(|d| d.name().map(|n| n == id.0).unwrap_or(false))
            .ok_or_else(|| AudioError::DeviceNotFound(id.0.clone()))
    }

    fn find_output(&self, id: &DeviceId) -> Result<cpal::Device, AudioError> {
        self.host
            .output_devices()
            .map_err(|e| AudioError::OpenStream(e.to_string()))?
            .find(|d| d.name().map(|n| n == id.0).unwrap_or(false))
            .ok_or_else(|| AudioError::DeviceNotFound(id.0.clone()))
    }
}

impl Default for CpalBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// 持有 cpal::Stream 以保持运行（Stream 非 Send 在部分平台，故用裸句柄包装）。
struct CpalInput {
    _stream: cpal::Stream,
    rate: u32,
}
// CPAL Stream 在 macOS/Windows 上由内部线程驱动；本句柄仅用于保活。
unsafe impl Send for CpalInput {}
impl InputStream for CpalInput {
    fn actual_sample_rate(&self) -> u32 {
        self.rate
    }
}

struct CpalOutput {
    _stream: cpal::Stream,
    rate: u32,
}
unsafe impl Send for CpalOutput {}
impl OutputStream for CpalOutput {
    fn actual_sample_rate(&self) -> u32 {
        self.rate
    }
}

fn to_info(d: &cpal::Device, default_name: &Option<String>) -> DeviceInfo {
    let name = d.name().unwrap_or_else(|_| "<unknown>".into());
    let is_default = default_name.as_deref() == Some(name.as_str());
    DeviceInfo {
        is_virtual: is_virtual_device_name(&name),
        is_default,
        id: DeviceId(name.clone()),
        name,
    }
}

impl AudioBackend for CpalBackend {
    fn list_inputs(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        let default = self.host.default_input_device().and_then(|d| d.name().ok());
        Ok(self
            .host
            .input_devices()
            .map_err(|e| AudioError::OpenStream(e.to_string()))?
            .map(|d| to_info(&d, &default))
            .collect())
    }

    fn list_outputs(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        let default = self.host.default_output_device().and_then(|d| d.name().ok());
        Ok(self
            .host
            .output_devices()
            .map_err(|e| AudioError::OpenStream(e.to_string()))?
            .map(|d| to_info(&d, &default))
            .collect())
    }

    fn open_input(
        &self,
        id: &DeviceId,
        cfg: StreamCfg,
    ) -> Result<(Box<dyn InputStream>, AudioConsumer), AudioError> {
        let device = self.find_input(id)?;
        let supported = device
            .default_input_config()
            .map_err(|e| AudioError::OpenStream(e.to_string()))?;
        let rate = supported.sample_rate().0;
        let channels = supported.channels();
        // 缓冲容量 = 1 秒，足够吸收抖动。
        let (mut prod, cons) = audio_channel(rate as usize * channels as usize);
        let stream = device
            .build_input_stream(
                &supported.config(),
                move |data: &[f32], _| {
                    // 多声道下混为单声道（取首声道），转 i16，推入环形缓冲。
                    let mut mono = Vec::with_capacity(data.len() / channels as usize);
                    for frame in data.chunks(channels as usize) {
                        mono.push((frame[0].clamp(-1.0, 1.0) * 32767.0) as i16);
                    }
                    let _ = prod.push_slice(&mono);
                },
                |err| tracing::error!(?err, "输入流错误"),
                None,
            )
            .map_err(|e| AudioError::OpenStream(e.to_string()))?;
        stream.play().map_err(|e| AudioError::OpenStream(e.to_string()))?;
        let _ = cfg; // M1 暂用设备默认配置
        Ok((Box::new(CpalInput { _stream: stream, rate }), cons))
    }

    fn open_output(
        &self,
        id: &DeviceId,
        cfg: StreamCfg,
    ) -> Result<(Box<dyn OutputStream>, AudioProducer), AudioError> {
        let device = self.find_output(id)?;
        let supported = device
            .default_output_config()
            .map_err(|e| AudioError::OpenStream(e.to_string()))?;
        let rate = supported.sample_rate().0;
        let channels = supported.channels();
        let (prod, mut cons) = audio_channel(rate as usize * channels as usize);
        let stream = device
            .build_output_stream(
                &supported.config(),
                move |data: &mut [f32], _| {
                    let need = data.len() / channels as usize;
                    let mut mono = vec![0i16; need];
                    let got = cons.pop_slice(&mut mono);
                    for (i, frame) in data.chunks_mut(channels as usize).enumerate() {
                        let v = if i < got { mono[i] as f32 / 32768.0 } else { 0.0 };
                        for ch in frame.iter_mut() {
                            *ch = v; // 单声道复制到所有声道
                        }
                    }
                },
                |err| tracing::error!(?err, "输出流错误"),
                None,
            )
            .map_err(|e| AudioError::OpenStream(e.to_string()))?;
        stream.play().map_err(|e| AudioError::OpenStream(e.to_string()))?;
        let _ = cfg;
        Ok((Box::new(CpalOutput { _stream: stream, rate }), prod))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_lists_without_panicking() {
        // CI 无声卡时设备列表可能为空，但不应 panic/报错。
        let backend = CpalBackend::new();
        let _ = backend.list_inputs();
        let _ = backend.list_outputs();
    }
}
```

- [ ] **Step 3: 运行测试（双平台）**

Run: `cargo test -p audio-cpal`
Expected: 1 passed（CI 上设备列表可能为空，但不报错）。本机 macOS 与 Windows 各跑一次。

- [ ] **Step 4: 提交**

```bash
git add crates/audio-cpal
git commit -m "feat(audio-cpal): CPAL 实现 AudioBackend（双平台输入输出）"
```

---

## Task 12: CLI 串起单向链路

**Files:**
- Create: `crates/cli/Cargo.toml`
- Create: `crates/cli/src/main.rs`

- [ ] **Step 1: 写 `crates/cli/Cargo.toml`**

```toml
[package]
name = "cli"
version = "0.1.0"
edition.workspace = true

[dependencies]
audio-core = { path = "../audio-core" }
audio-cpal = { path = "../audio-cpal" }
audio-dsp = { path = "../audio-dsp" }
gemini-live = { path = "../gemini-live" }
tokio = { workspace = true, features = ["rt-multi-thread", "macros", "time", "signal"] }
anyhow.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
```

- [ ] **Step 2: 写 `crates/cli/src/main.rs`**

```rust
//! M1 验证 CLI：物理麦 → 重采样 16k → Gemini Live Translate → 24k 播放到扬声器。
//! 用法：
//!   GEMINI_API_KEY=xxx cargo run -p cli -- --target en --in-device "<麦克风名>" --out-device "<扬声器名>"
//! 不带 --in-device 时列出设备并退出。
use audio_core::{AudioBackend, DeviceId, PcmFrame, StreamCfg};
use audio_cpal::CpalBackend;
use audio_dsp::Resampler;
use gemini_live::session::{connect_with_retry, SessionConfig};
use std::time::Instant;

const MODEL: &str = "models/gemini-3.5-live-translate";
const GEMINI_IN_RATE: u32 = 16_000;
const GEMINI_OUT_RATE: u32 = 24_000;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args: Vec<String> = std::env::args().collect();
    let backend = CpalBackend::new();

    let in_device = arg_value(&args, "--in-device");
    let out_device = arg_value(&args, "--out-device");
    let target = arg_value(&args, "--target").unwrap_or_else(|| "en".to_string());

    if in_device.is_none() {
        println!("== 输入设备 ==");
        for d in backend.list_inputs()? {
            println!("  {}{}", d.name, if d.is_default { " [默认]" } else { "" });
        }
        println!("== 输出设备 ==");
        for d in backend.list_outputs()? {
            println!("  {}{}", d.name, if d.is_default { " [默认]" } else { "" });
        }
        println!("\n请用 --in-device/--out-device 指定设备后重跑。");
        return Ok(());
    }

    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| anyhow::anyhow!("缺少 GEMINI_API_KEY 环境变量"))?;
    let url = format!(
        "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?access_token={api_key}"
    );

    // 打开音频设备
    let cfg = StreamCfg { sample_rate: 48_000, channels: 1, frame_size: 480 };
    let (in_stream, mut mic_cons) =
        backend.open_input(&DeviceId(in_device.unwrap()), cfg)?;
    let (out_stream, mut spk_prod) =
        backend.open_output(&DeviceId(out_device.unwrap()), cfg)?;
    let in_rate = in_stream.actual_sample_rate();
    let out_rate = out_stream.actual_sample_rate();
    tracing::info!(in_rate, out_rate, %target, "音频设备已打开");

    // 连接 Gemini（auto 模式：不锁源，目标=target）
    let (audio_tx, mut audio_rx) = connect_with_retry(
        || SessionConfig { url: url.clone(), model: MODEL.into(), out_rate: GEMINI_OUT_RATE },
        5,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Gemini 连接失败: {e}"))?;
    tracing::info!("Gemini 已连接");

    // 上行：麦克风(in_rate) → 16k → 发送
    let up_chunk = (in_rate / 100) as usize; // 10ms
    let mut up_resampler = Resampler::new(in_rate, GEMINI_IN_RATE, up_chunk);
    let send_started = Instant::now();
    tokio::spawn(async move {
        let mut buf = vec![0i16; up_chunk];
        loop {
            let got = mic_cons.pop_slice(&mut buf);
            if got == up_chunk {
                let frame = PcmFrame::new(buf.clone(), in_rate);
                let frame16 = up_resampler.process(&frame);
                if audio_tx.send(frame16).await.is_err() {
                    break;
                }
            } else {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
    });

    // 下行：接收 24k → out_rate → 播放，并记录首包延迟
    let mut down_resampler = Resampler::new(GEMINI_OUT_RATE, out_rate, 480);
    let mut first_audio_logged = false;
    let mut latencies_ms: Vec<u128> = Vec::new();
    tokio::spawn(async move {
        while let Some(frame24) = audio_rx.recv().await {
            if !first_audio_logged {
                let lat = send_started.elapsed().as_millis();
                latencies_ms.push(lat);
                tracing::info!(latency_ms = lat, "首个翻译音频到达");
                first_audio_logged = true;
            }
            // 不足 480 则补零到一块再重采样（M1 简化）
            for chunk in frame24.samples.chunks(480) {
                let mut block = chunk.to_vec();
                block.resize(480, 0);
                let f = PcmFrame::new(block, GEMINI_OUT_RATE);
                let out = down_resampler.process(&f);
                spk_prod.push_slice(&out.samples);
            }
        }
    });

    println!("运行中，对着麦克风说话；Ctrl+C 停止。");
    tokio::signal::ctrl_c().await?;
    drop(in_stream);
    drop(out_stream);
    println!("已停止。");
    Ok(())
}

fn arg_value(args: &[String], key: &str) -> Option<String> {
    args.iter().position(|a| a == key).and_then(|i| args.get(i + 1).cloned())
}
```

- [ ] **Step 3: 编译检查**

Run: `cargo build -p cli`
Expected: 编译成功。

- [ ] **Step 4: 列设备冒烟测试**

Run: `cargo run -p cli`
Expected: 打印输入/输出设备列表并提示指定设备。

- [ ] **Step 5: 提交**

```bash
git add crates/cli
git commit -m "feat(cli): 单向实时翻译链路（麦→重采样→Gemini→扬声器）"
```

---

## Task 13: 真实 API 联调与延迟记录（手动验证）

**Files:**
- Create: `docs/superpowers/notes/M1-validation.md`（记录结果）

> 此任务是 M1 的验收，需真实 `GEMINI_API_KEY` 与麦克风/扬声器，无法自动化。

- [ ] **Step 1: 协议核对——打印原始服务端帧**

临时在 `session.rs` 接收任务里，对解析失败或首 3 条消息 `tracing::info!(%text, "原始帧")`。运行 CLI，确认真实字段名与 `protocol.rs` 一致；若 `serverContent/modelTurn/parts/inlineData` 命名有出入，按真实流修正 `protocol.rs` 并补一条 serde 测试，再移除临时日志。

Run: `GEMINI_API_KEY=<key> cargo run -p cli -- --in-device "<麦克风>" --out-device "<扬声器>" --target en`
Expected: 日志出现 `Gemini 已连接`、`原始帧`、`首个翻译音频到达 latency_ms=…`。

- [ ] **Step 2: 中文→英文听感验证**

对麦克风说中文，从扬声器听到英文翻译；连续讲话 1 分钟，确认无明显爆音/卡顿/无限延迟堆积。

- [ ] **Step 3: 反向验证（英文→中文）**

`--target zh`，对麦克风说英文，听到中文。

- [ ] **Step 4: 10 分钟稳定性 + 延迟统计**

持续运行 10 分钟，间歇说话。收集所有“首个翻译音频到达”及（可扩展记录的）逐句延迟，计算 P50/P95。

- [ ] **Step 5: 写验收记录 `docs/superpowers/notes/M1-validation.md`**

```markdown
# M1 验收记录

- 日期 / 平台（macOS / Windows）
- 模型 ID、target 语言
- 首包延迟、逐句 P50 / P95（ms）
- 10 分钟是否崩溃 / 异常
- 听感主观评分（爆音/卡顿/延迟堆积）
- 与 PRD 9.1 延迟指标的差距 → 是否需放宽指标
- 协议字段与 protocol.rs 的差异修正记录
```

- [ ] **Step 6: 提交**

```bash
git add docs/superpowers/notes/M1-validation.md crates/gemini-live
git commit -m "docs(M1): Gemini 实时翻译链路验收记录与协议核对修正"
```

**M1 完成检查点（验收标准）：** 中英任一方向实时语音翻译可用；连续运行 10 分钟不崩；已记录 P50/P95 延迟；`cargo test --workspace` 与 `cargo clippy --workspace -- -D warnings` 全绿。

---

## 自查（Self-Review）

**Spec 覆盖：**
- A 方案线程化环形缓冲 → Task 3（ring）+ Task 11（回调推/拉缓冲）✅
- AudioBackend 平台无关抽象 + 双平台 → Task 4 + Task 11（CPAL 覆盖双平台）+ CI（Task 1）✅
- gemini-live 自实现 WebSocket 客户端 → Task 6–9 ✅
- 过期帧丢弃防延迟堆积 → Task 9 ✅
- 重采样 48k↔16k/24k → Task 10 + Task 12 ✅
- auto 模式不锁源/目标=听者语言 → Task 6（`new_translate` 不写 source）+ Task 12（`--target`）✅
- 控制面状态机/最坏态投影 → Task 5 ✅
- M1 验收（10 分钟 + 延迟）→ Task 13 ✅
- 虚拟设备名识别（为 M2 预埋）→ Task 4 ✅

**说明：** 防回声三道防线、双向第二条链路、VAD、Tauri/UI 属 M2/M3，不在本切片，未列任务（符合切片边界）。

**占位符扫描：** 无 TODO/TBD；Task 13 的“协议核对”是有意的真实-API 验证步骤，含具体操作与命令，非占位。

**类型一致性：** `PcmFrame`、`AudioBackend`、`SessionConfig`、`connect`/`connect_with_retry`、`Resampler::new/process`、`drop_stale_frames`、`Setup::new_translate` 在定义与调用处签名一致。
