# M2 双向链路 + 设备管理 + 防循环 + VAD 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Task 编号承接 M0/M1（止于 Task 13），M2 从 **Task 14** 起。

**Goal:** 在 M1 单向链路基础上，点亮第二条独立下行链路，交付 Zoom/Teams 真机中英双向通话能力。新增 `device-manager`（设备枚举/分类/热插拔）与 `diagnostics`（物理隔离校验 + 回环检测自动暂停）两个平台无关 crate；在 `engine` 内落成路由矩阵（双向/仅上行/仅下行）与双链路编排器；在 `audio-dsp` 落成 VAD 省成本；CLI 升级为驱动 engine 编排器的薄壳。

**Architecture:** 严格延续 M1 的控制面/数据面分离与双平台纪律。**所有新增逻辑（设备分类、快照 diff、路由矩阵、隔离校验、回环检测、状态机、VAD）必须是平台无关、可单测的纯函数或纯类型**，落在 `device-manager` / `diagnostics` / `engine` / `audio-dsp`。平台差异（cpal 真实枚举、热插拔轮询）只允许新增在 `audio-cpal`。`engine` 及以上**禁止** `#[cfg(target_os)]`。数据面热路径（音频回调、push/pop slice）保持零分配/零锁/零 await；逐帧能量摘要与 VAD 判定按 `&[i16]` 借用、零堆分配。第二条下行链路 = 再 `connect` 一个独立 Session（一条 Session = 一条下行链路，类型已就绪），独立 Tokio task、独立重连、独立背压、独立过期帧丢弃。

**Tech Stack:** 沿用 M1（Rust 2021、CPAL、rubato、tokio + tokio-tungstenite、serde、ringbuf、tracing、thiserror、anyhow）。M2 不引入新的第三方依赖：VAD/能量/回环检测全部整数运算 + std；device-manager 仅依赖 `audio-core`；diagnostics 仅依赖 `audio-core`；engine 新增 `gemini-live` / `audio-dsp` / `diagnostics` / `device-manager` / `tokio` 路径与 workspace 依赖。AEC、降噪、Tauri/UI 滞后 M3，不在本切片。

---

## 文件结构（M2 新增/修改）

```
translate/
├── crates/
│   ├── audio-core/
│   │   └── src/
│   │       ├── lib.rs              # [改] re-export watch_devices 相关新类型
│   │       └── backend.rs          # [改] 破坏性新增 watch_devices()->DeviceWatchHandle；RawDeviceEvent/Direction
│   ├── audio-cpal/
│   │   └── src/lib.rs              # [改] 实现 watch_devices() 轮询线程（~1s，复用 list_with_timeout，空列表保护）
│   ├── audio-dsp/
│   │   └── src/
│   │       ├── lib.rs              # [改] pub mod vad
│   │       └── vad.rs              # [新] 能量+过零率 VAD + 滞回状态机 + VadStats（纯函数/零分配）
│   ├── device-manager/            # [新 crate]
│   │   ├── Cargo.toml              # deps 仅 audio-core + thiserror；严禁 cpal
│   │   └── src/
│   │       ├── lib.rs             # 导出 DeviceUse/Direction/DeviceSnapshot/DeviceEvent/DeviceManager
│   │       ├── classify.rs        # classify(info, dir)->DeviceUse 纯函数
│   │       ├── snapshot.rs        # DeviceSnapshot + diff_snapshots 纯函数
│   │       ├── watch.rs           # DeviceEvent/Direction + project_lost 纯函数
│   │       └── manager.rs         # DeviceManager<B:AudioBackend> 不可变快照查询
│   ├── diagnostics/               # [新 crate]
│   │   ├── Cargo.toml             # deps 仅 audio-core；严禁 cpal / #[cfg(target_os)]
│   │   └── src/
│   │       ├── lib.rs            # 导出 isolation/loopcheck/meter
│   │       ├── meter.rs         # frame_energy(&[i16])->FrameEnergy 零分配
│   │       ├── isolation.rs    # validate_isolation(&[LinkRoute]) 纯函数（第一道防线）
│   │       └── loopcheck.rs    # detect_loop + step_guard 滞回状态机（第二道防线）
│   ├── engine/
│   │   ├── Cargo.toml             # [改] 新增 gemini-live/audio-dsp/diagnostics/device-manager/tokio/thiserror
│   │   └── src/
│   │       ├── lib.rs            # [改] pub mod route/link/orchestrator
│   │       ├── control.rs       # [改] 新增 SessionState::Paused、ControlEvent::{LoopSuspected,TranslationPaused,TranslationResumed}、PauseReason
│   │       ├── route.rs         # [新] RouteMatrix/LinkRole/build_routes/validate_isolation/active_links 纯函数
│   │       ├── link.rs          # [新] LinkHandle/spawn_link 单链路独立运行单元
│   │       └── orchestrator.rs  # [新] Orchestrator 持 0..2 LinkHandle，worst_state 投影发事件
│   └── cli/
│       ├── Cargo.toml            # [改] 新增 engine/device-manager 依赖
│       └── src/main.rs          # [改] 升级为薄壳：--mode/双链路参数、DeviceManager 分类列表、构建 RouteSpec、Orchestrator
```

---

# 阶段 M2 · 双向链路 + 设备管理 + 防循环 + VAD

> **实现顺序原则**：先做不依赖跨 crate 的纯函数 crate（device-manager、diagnostics、VAD），再做 trait 破坏性扩展与 cpal 实现，最后做 engine 编排与 CLI 接线。每个 Task 自测全绿再进下一 Task。

---

## Task 14: audio-core trait 破坏性扩展 `watch_devices`

**Files:**
- Modify: `crates/audio-core/src/backend.rs`
- Modify: `crates/audio-core/src/lib.rs`

- [ ] **Step 1: 在 `backend.rs` 新增平台无关原始事件类型与订阅句柄（含测试）**

```rust
/// 设备方向（输入=采集，输出=播放）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Input,
    Output,
}

/// 后端发出的原始设备变更事件：发完整快照，语义 diff 交给 device-manager 纯函数。
#[derive(Debug, Clone, PartialEq)]
pub enum RawDeviceEvent {
    ListChanged {
        inputs: Vec<DeviceInfo>,
        outputs: Vec<DeviceInfo>,
    },
}

/// 设备变更订阅句柄。后端在独立线程推送 RawDeviceEvent，上层低频轮询消费。
pub struct DeviceWatchHandle {
    rx: std::sync::mpsc::Receiver<RawDeviceEvent>,
}

impl DeviceWatchHandle {
    pub fn new(rx: std::sync::mpsc::Receiver<RawDeviceEvent>) -> Self {
        Self { rx }
    }
    pub fn try_recv(&self) -> Option<RawDeviceEvent> {
        self.rx.try_recv().ok()
    }
    pub fn recv_timeout(&self, d: std::time::Duration) -> Option<RawDeviceEvent> {
        self.rx.recv_timeout(d).ok()
    }
}
```

并在 `AudioBackend` trait 末尾新增方法（破坏性，audio-cpal 须在 Task 22 同步实现）：

```rust
    /// 订阅设备增删变更。返回低频控制事件通道句柄。
    fn watch_devices(&self) -> Result<DeviceWatchHandle, AudioError>;
```

- [ ] **Step 2: 测试（加在 backend.rs 测试段）**

```rust
    #[test]
    fn watch_handle_relays_raw_event() {
        let (tx, rx) = std::sync::mpsc::channel();
        let h = DeviceWatchHandle::new(rx);
        assert!(h.try_recv().is_none());
        tx.send(RawDeviceEvent::ListChanged { inputs: vec![], outputs: vec![] })
            .unwrap();
        assert!(matches!(h.try_recv(), Some(RawDeviceEvent::ListChanged { .. })));
    }
```

- [ ] **Step 3: 在 `lib.rs` 导出新类型**

```rust
pub use backend::{Direction, RawDeviceEvent, DeviceWatchHandle};
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p audio-core backend`
Expected: 原有 + 新增 watch 测试全 passed。注意此步会让 `audio-cpal` 暂时编译失败（trait 缺方法实现），属预期——Task 22 补齐前不要跑 `--workspace`，先 `cargo test -p audio-core`。

- [ ] **Step 5: 提交**

```bash
git add crates/audio-core
git commit -m "feat(audio-core): AudioBackend 新增 watch_devices 与原始设备事件类型"
```

---

## Task 15: device-manager 设备分类纯函数

**Files:**
- Create: `crates/device-manager/Cargo.toml`
- Create: `crates/device-manager/src/lib.rs`
- Create: `crates/device-manager/src/classify.rs`

- [ ] **Step 1: 写 `crates/device-manager/Cargo.toml`（严禁 cpal）**

```toml
[package]
name = "device-manager"
version = "0.1.0"
edition.workspace = true

[dependencies]
audio-core = { path = "../audio-core" }
thiserror.workspace = true
```

- [ ] **Step 2: 写 `classify.rs`（纯函数 + 测试）**

```rust
//! 设备用途分类：用途由 方向 + is_virtual 共同决定。
//! VirtualMic = 注入汇（Output 方向的虚拟设备，把译文写进去喂给会议软件）；
//! VirtualSpeaker = 采集源（Input 方向的虚拟设备，从中采集远端音频）；
//! Physical = 真实物理麦/耳机。
use audio_core::{DeviceInfo, Direction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceUse {
    VirtualMic,
    VirtualSpeaker,
    Physical,
}

pub fn classify(info: &DeviceInfo, dir: Direction) -> DeviceUse {
    match (info.is_virtual, dir) {
        (true, Direction::Output) => DeviceUse::VirtualMic,
        (true, Direction::Input) => DeviceUse::VirtualSpeaker,
        (false, _) => DeviceUse::Physical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_core::DeviceId;

    fn dev(name: &str, virt: bool) -> DeviceInfo {
        DeviceInfo { id: DeviceId(name.into()), name: name.into(), is_default: false, is_virtual: virt }
    }

    #[test]
    fn classify_uses_direction_plus_virtual_name() {
        assert_eq!(
            classify(&dev("CABLE Input (VB-Audio Virtual Cable)", true), Direction::Output),
            DeviceUse::VirtualMic
        );
        assert_eq!(classify(&dev("BlackHole 2ch", true), Direction::Input), DeviceUse::VirtualSpeaker);
        assert_eq!(classify(&dev("MacBook Pro Microphone", false), Direction::Input), DeviceUse::Physical);
    }
}
```

- [ ] **Step 3: 写 `lib.rs`（先只挂 classify，后续 Task 追加）**

```rust
//! 平台无关设备管理：枚举/分类/快照 diff/热插拔投影。仅依赖 audio-core trait，零 cpal。
pub mod classify;
pub use classify::{classify, DeviceUse};
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p device-manager classify`
Expected: 1 passed。

- [ ] **Step 5: 提交**

```bash
git add crates/device-manager
git commit -m "feat(device-manager): 设备用途分类纯函数（方向+虚拟名）"
```

---

## Task 16: device-manager 快照 diff 纯函数

**Files:**
- Create: `crates/device-manager/src/snapshot.rs`
- Create: `crates/device-manager/src/watch.rs`
- Modify: `crates/device-manager/src/lib.rs`

- [ ] **Step 1: 写 `watch.rs`（事件类型 + project_lost 纯函数）**

```rust
//! 设备语义事件与「正在使用却消失」投影。
use audio_core::{DeviceId, Direction};
use crate::snapshot::DeviceSnapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceEvent {
    Added { id: DeviceId, dir: Direction },
    Removed { id: DeviceId, dir: Direction },
    DefaultChanged { dir: Direction, new: DeviceId },
    DeviceLost { id: DeviceId, dir: Direction },
}

/// 正在使用的设备在新快照中缺失 -> DeviceLost（升级为 Error 子态，区别于普通 Removed）。
pub fn project_lost(in_use: &[(DeviceId, Direction)], next: &DeviceSnapshot) -> Vec<DeviceEvent> {
    in_use
        .iter()
        .filter(|(id, dir)| !next.contains(id, *dir))
        .map(|(id, dir)| DeviceEvent::DeviceLost { id: id.clone(), dir: *dir })
        .collect()
}
```

- [ ] **Step 2: 写 `snapshot.rs`（不可变快照 + diff_snapshots 纯函数 + 测试）**

```rust
//! 设备快照与前后 diff。快照不可变：刷新产生新 Vec，绝不原地 mutate。
use audio_core::{DeviceId, DeviceInfo, Direction};
use crate::watch::DeviceEvent;

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceSnapshot {
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
}

impl DeviceSnapshot {
    pub fn contains(&self, id: &DeviceId, dir: Direction) -> bool {
        let list = match dir { Direction::Input => &self.inputs, Direction::Output => &self.outputs };
        list.iter().any(|d| &d.id == id)
    }
    fn default_of(list: &[DeviceInfo]) -> Option<&DeviceId> {
        list.iter().find(|d| d.is_default).map(|d| &d.id)
    }
}

/// 前后快照 diff：Added/Removed/DefaultChanged。纯函数，不 mutate 入参。
pub fn diff_snapshots(prev: &DeviceSnapshot, next: &DeviceSnapshot) -> Vec<DeviceEvent> {
    let mut out = Vec::new();
    for (dir, p, n) in [
        (Direction::Input, &prev.inputs, &next.inputs),
        (Direction::Output, &prev.outputs, &next.outputs),
    ] {
        for d in n { if !p.iter().any(|x| x.id == d.id) {
            out.push(DeviceEvent::Added { id: d.id.clone(), dir });
        }}
        for d in p { if !n.iter().any(|x| x.id == d.id) {
            out.push(DeviceEvent::Removed { id: d.id.clone(), dir });
        }}
        let pd = DeviceSnapshot::default_of(p);
        let nd = DeviceSnapshot::default_of(n);
        if pd != nd {
            if let Some(new) = nd {
                out.push(DeviceEvent::DefaultChanged { dir, new: new.clone() });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::project_lost;

    fn d(name: &str, def: bool) -> DeviceInfo {
        DeviceInfo { id: DeviceId(name.into()), name: name.into(), is_default: def, is_virtual: false }
    }
    fn snap(ins: Vec<DeviceInfo>, outs: Vec<DeviceInfo>) -> DeviceSnapshot {
        DeviceSnapshot { inputs: ins, outputs: outs }
    }
    fn sorted(mut v: Vec<DeviceEvent>) -> Vec<DeviceEvent> {
        v.sort_by_key(|e| format!("{e:?}")); v
    }

    #[test]
    fn diff_added_and_removed_set() {
        let prev = snap(vec![d("A", false), d("B", false)], vec![]);
        let next = snap(vec![d("B", false), d("C", false)], vec![]);
        let got = sorted(diff_snapshots(&prev, &next));
        assert_eq!(got, sorted(vec![
            DeviceEvent::Added { id: DeviceId("C".into()), dir: Direction::Input },
            DeviceEvent::Removed { id: DeviceId("A".into()), dir: Direction::Input },
        ]));
    }

    #[test]
    fn diff_default_change_only() {
        let prev = snap(vec![d("A", true), d("B", false)], vec![]);
        let next = snap(vec![d("A", false), d("B", true)], vec![]);
        assert_eq!(diff_snapshots(&prev, &next), vec![
            DeviceEvent::DefaultChanged { dir: Direction::Input, new: DeviceId("B".into()) },
        ]);
    }

    #[test]
    fn project_lost_only_for_in_use() {
        let next = snap(vec![d("SpkY", false)], vec![]);
        let in_use = vec![
            (DeviceId("MicX".into()), Direction::Input),
            (DeviceId("SpkY".into()), Direction::Input),
        ];
        assert_eq!(project_lost(&in_use, &next), vec![
            DeviceEvent::DeviceLost { id: DeviceId("MicX".into()), dir: Direction::Input },
        ]);
        let still = vec![(DeviceId("SpkY".into()), Direction::Input)];
        assert!(project_lost(&still, &next).is_empty());
    }
}
```

- [ ] **Step 3: 在 `lib.rs` 导出**

```rust
pub mod snapshot;
pub mod watch;
pub use snapshot::{diff_snapshots, DeviceSnapshot};
pub use watch::{project_lost, DeviceEvent};
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p device-manager`
Expected: classify 1 + snapshot/watch 3 = 4 passed。

- [ ] **Step 5: 提交**

```bash
git add crates/device-manager
git commit -m "feat(device-manager): 快照 diff 与 DeviceLost 投影纯函数"
```

---

## Task 17: device-manager 管理器（基于 mock backend）

**Files:**
- Create: `crates/device-manager/src/manager.rs`
- Modify: `crates/device-manager/src/lib.rs`

- [ ] **Step 1: 写 `manager.rs`（DeviceManager + mock backend 测试）**

```rust
//! DeviceManager：持有不可变 current 快照，提供 refresh/查询。不在数据面热路径。
use audio_core::{AudioBackend, AudioError, DeviceId, DeviceInfo, Direction};
use crate::snapshot::DeviceSnapshot;
use crate::classify::{classify, DeviceUse};

pub struct DeviceManager<B: AudioBackend> {
    backend: B,
    current: DeviceSnapshot,
}

impl<B: AudioBackend> DeviceManager<B> {
    pub fn new(backend: B) -> Result<Self, AudioError> {
        let current = DeviceSnapshot {
            inputs: backend.list_inputs()?,
            outputs: backend.list_outputs()?,
        };
        Ok(Self { backend, current })
    }

    /// 重新枚举，返回新快照（不原地 mutate 旧快照内容，整体替换为新 Vec）。
    pub fn refresh(&mut self) -> Result<DeviceSnapshot, AudioError> {
        let next = DeviceSnapshot {
            inputs: self.backend.list_inputs()?,
            outputs: self.backend.list_outputs()?,
        };
        self.current = next.clone();
        Ok(next)
    }

    pub fn snapshot(&self) -> &DeviceSnapshot { &self.current }

    pub fn default_for(&self, dir: Direction) -> Option<&DeviceInfo> {
        let list = match dir { Direction::Input => &self.current.inputs, Direction::Output => &self.current.outputs };
        list.iter().find(|d| d.is_default)
    }

    pub fn pick_use(&self, u: DeviceUse) -> Vec<&DeviceInfo> {
        let mut out = Vec::new();
        for d in &self.current.inputs { if classify(d, Direction::Input) == u { out.push(d); } }
        for d in &self.current.outputs { if classify(d, Direction::Output) == u { out.push(d); } }
        out
    }

    pub fn resolve(&self, id: &DeviceId, dir: Direction) -> Option<&DeviceInfo> {
        let list = match dir { Direction::Input => &self.current.inputs, Direction::Output => &self.current.outputs };
        list.iter().find(|d| &d.id == id)
    }
}
```

测试段（实现一个最小 MockBackend 覆盖 AudioBackend，其中 open_*/watch_devices 可 unimplemented! 或返回空，list_* 用内部 Cell/RefCell 切换返回值证明 refresh 产生新快照）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use audio_core::{AudioConsumer, AudioProducer, DeviceWatchHandle, InputStream, OutputStream, StreamCfg};
    use std::cell::RefCell;

    fn d(name: &str, def: bool, virt: bool) -> DeviceInfo {
        DeviceInfo { id: DeviceId(name.into()), name: name.into(), is_default: def, is_virtual: virt }
    }

    struct MockBackend { inputs: RefCell<Vec<DeviceInfo>>, outputs: RefCell<Vec<DeviceInfo>> }
    // SAFETY: 测试单线程使用；AudioBackend 要求 Send+Sync，RefCell 在测试上下文不跨线程。
    unsafe impl Sync for MockBackend {}
    impl AudioBackend for MockBackend {
        fn list_inputs(&self) -> Result<Vec<DeviceInfo>, AudioError> { Ok(self.inputs.borrow().clone()) }
        fn list_outputs(&self) -> Result<Vec<DeviceInfo>, AudioError> { Ok(self.outputs.borrow().clone()) }
        fn open_input(&self, _: &DeviceId, _: StreamCfg) -> Result<(Box<dyn InputStream>, AudioConsumer), AudioError> { unimplemented!() }
        fn open_output(&self, _: &DeviceId, _: StreamCfg) -> Result<(Box<dyn OutputStream>, AudioProducer), AudioError> { unimplemented!() }
        fn watch_devices(&self) -> Result<DeviceWatchHandle, AudioError> {
            let (_tx, rx) = std::sync::mpsc::channel();
            Ok(DeviceWatchHandle::new(rx))
        }
    }

    #[test]
    fn refresh_returns_new_immutable_snapshot() {
        let backend = MockBackend {
            inputs: RefCell::new(vec![d("MicX", true, false)]),
            outputs: RefCell::new(vec![d("BlackHole 2ch", false, true)]),
        };
        let mut mgr = DeviceManager::new(backend).unwrap();
        let old = mgr.snapshot().clone();
        assert_eq!(old.inputs.len(), 1);
        // 改 mock：新增一个虚拟扬声器作为采集源（Input 方向虚拟设备）
        mgr.backend.inputs.borrow_mut().push(d("BlackHole 16ch", false, true));
        let new = mgr.refresh().unwrap();
        assert_ne!(old, new, "refresh 必须产生新快照，未原地 mutate");
        // 端到端：原始变化经 diff_snapshots -> 语义 Added，且 classify 为 VirtualSpeaker
        let events = crate::snapshot::diff_snapshots(&old, &new);
        assert!(events.iter().any(|e| matches!(e, crate::watch::DeviceEvent::Added { dir: Direction::Input, .. })));
        assert_eq!(classify(&new.inputs[1], Direction::Input), DeviceUse::VirtualSpeaker);
    }
}
```

- [ ] **Step 2: 在 `lib.rs` 导出 + re-export Direction**

```rust
pub mod manager;
pub use manager::DeviceManager;
pub use audio_core::Direction;
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p device-manager`
Expected: 5 passed（含 refresh 不可变 + 原始事件经纯函数到语义事件，全程无 cpal）。

- [ ] **Step 4: 提交**

```bash
git add crates/device-manager
git commit -m "feat(device-manager): DeviceManager 不可变快照查询与刷新（mock backend 验证）"
```

---

## Task 18: diagnostics 逐帧能量摘要 `meter`

**Files:**
- Create: `crates/diagnostics/Cargo.toml`
- Create: `crates/diagnostics/src/lib.rs`
- Create: `crates/diagnostics/src/meter.rs`

- [ ] **Step 1: 写 `crates/diagnostics/Cargo.toml`**

```toml
[package]
name = "diagnostics"
version = "0.1.0"
edition.workspace = true

[dependencies]
audio-core = { path = "../audio-core" }
```

- [ ] **Step 2: 写 `meter.rs`（零分配逐帧摘要 + 测试）**

```rust
//! 逐帧能量摘要：数据面热路径调用，O(n) 累加、无堆分配、无锁、无 await。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameEnergy {
    pub rms_q15: i32, // RMS 幅度（与样本同量纲），整数实现
    pub peak: i16,
    pub n: u16,
}

pub fn frame_energy(samples: &[i16]) -> FrameEnergy {
    if samples.is_empty() {
        return FrameEnergy { rms_q15: 0, peak: 0, n: 0 };
    }
    let mut acc: i64 = 0;
    let mut peak: i16 = 0;
    for &s in samples {
        acc += (s as i64) * (s as i64);
        let a = s.unsigned_abs() as i16; // |s|，饱和处理 i16::MIN
        if a > peak { peak = a; }
    }
    let mean = acc / samples.len() as i64;
    let rms = (mean as f64).sqrt() as i32; // 仅一次标量 sqrt，无堆分配
    FrameEnergy { rms_q15: rms, peak, n: samples.len() as u16 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_energy_is_zero_alloc_and_correct() {
        assert_eq!(frame_energy(&[0i16; 480]), FrameEnergy { rms_q15: 0, peak: 0, n: 480 });
        let e = frame_energy(&[i16::MAX; 480]);
        assert_eq!(e.peak, i16::MAX);
        assert!(e.rms_q15 > 32000, "满刻度 RMS 接近 i16::MAX, got {}", e.rms_q15);
        assert_eq!(frame_energy(&[]).n, 0); // 空切片不 panic
    }
}
```

- [ ] **Step 3: 写 `lib.rs`**

```rust
//! 诊断：物理隔离校验（第一道防线）+ 回环检测自动暂停（第二道防线）+ 能量摘要。
//! 平台无关、纯函数、热路径零分配。仅依赖 audio-core，零 cpal、零 #[cfg(target_os)]。
pub mod meter;
pub use meter::{frame_energy, FrameEnergy};
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p diagnostics meter`
Expected: 1 passed。

- [ ] **Step 5: 提交**

```bash
git add crates/diagnostics
git commit -m "feat(diagnostics): 逐帧能量摘要 frame_energy（零分配）"
```

---

## Task 19: diagnostics 物理隔离校验（第一道防线）

**Files:**
- Create: `crates/diagnostics/src/isolation.rs`
- Modify: `crates/diagnostics/src/lib.rs`

- [ ] **Step 1: 写 `isolation.rs`（纯函数 + 测试）**

```rust
//! 第一道防线：启动期结构性隔离。任一链路的「汇」不得等于任一链路（含自身/对侧）的「源」，
//! 且翻译输出汇不得指向被用作采集源的虚拟设备（防自激环）。纯函数，可单测。
use audio_core::DeviceId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRoute {
    pub source: DeviceId,
    pub sink: DeviceId,
    pub source_is_virtual: bool,
    pub sink_is_virtual: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IsolationError {
    SourceSinkOverlap { device: String },
    OutputIsVirtualCaptureSource { device: String },
}

pub fn validate_isolation(links: &[LinkRoute]) -> Result<(), IsolationError> {
    let sources: Vec<&DeviceId> = links.iter().map(|l| &l.source).collect();
    for l in links {
        // 汇 == 任一源（含自身）-> 结构性回环
        if sources.iter().any(|s| **s == l.sink) {
            // 输出指向某个虚拟采集源 -> 更明确的错误
            let sink_used_as_source = links.iter().any(|x| x.source == l.sink && x.source_is_virtual);
            if l.sink_is_virtual && sink_used_as_source {
                return Err(IsolationError::OutputIsVirtualCaptureSource { device: l.sink.0.clone() });
            }
            return Err(IsolationError::SourceSinkOverlap { device: l.sink.0.clone() });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(s: &str) -> DeviceId { DeviceId(s.into()) }

    #[test]
    fn isolation_rejects_same_device_as_source_and_sink() {
        let bad = [LinkRoute { source: id("BlackHole"), sink: id("BlackHole"), source_is_virtual: true, sink_is_virtual: true }];
        assert!(matches!(validate_isolation(&bad), Err(IsolationError::OutputIsVirtualCaptureSource { .. })));
        // 合法双链路：四设备互不重叠
        let ok = [
            LinkRoute { source: id("PhysMic"), sink: id("VirtMic"), source_is_virtual: false, sink_is_virtual: true },
            LinkRoute { source: id("VirtSpk"), sink: id("PhysHeadset"), source_is_virtual: true, sink_is_virtual: false },
        ];
        assert_eq!(validate_isolation(&ok), Ok(()));
    }

    #[test]
    fn isolation_rejects_output_to_virtual_capture_source() {
        // 上行汇 == 下行采集源（同一虚拟设备）-> OutputIsVirtualCaptureSource
        let bad = [
            LinkRoute { source: id("PhysMic"), sink: id("VirtSpk"), source_is_virtual: false, sink_is_virtual: true },
            LinkRoute { source: id("VirtSpk"), sink: id("PhysHeadset"), source_is_virtual: true, sink_is_virtual: false },
        ];
        assert!(matches!(validate_isolation(&bad), Err(IsolationError::OutputIsVirtualCaptureSource { .. })));
    }
}
```

- [ ] **Step 2: 在 `lib.rs` 导出**

```rust
pub mod isolation;
pub use isolation::{validate_isolation, IsolationError, LinkRoute};
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p diagnostics isolation`
Expected: 2 passed。

- [ ] **Step 4: 提交**

```bash
git add crates/diagnostics
git commit -m "feat(diagnostics): 物理隔离校验纯函数（防循环第一道防线）"
```

---

## Task 20: diagnostics 回环检测与滞回状态机（第二道防线）

**Files:**
- Create: `crates/diagnostics/src/loopcheck.rs`
- Modify: `crates/diagnostics/src/lib.rs`

- [ ] **Step 1: 写 `loopcheck.rs`（detect_loop + step_guard + 测试）**

> Host wiring（注释说明）：数据面把注入端与采集端的 `FrameEnergy` 经 ringbuf SPSC 通道（与 `audio_channel` 同构，单生产单消费）喂给控制面的 detector task；detector 维护滑窗，调用 `detect_loop` 得证据，再 `step_guard` 走滞回，产出 `Pause/Resume` 动作写 `LoopPauseFlag`（Task 24 接线）。本文件只含纯函数，不持 IO。

```rust
//! 第二道防线：回环检测 + 滞回状态机。纯函数，无状态/无 IO。
use crate::meter::FrameEnergy;

#[derive(Debug, Clone, Copy)]
pub struct LoopThresholds {
    pub energy_ratio_db: f32, // 采集/注入能量比下限（高于此才可能是回声）
    pub min_xcorr: f32,       // 归一化跨相关下限
    pub max_lag_frames: u16,  // 搜索的最大延迟
    pub hold_frames: u16,     // 连续疑似多少帧才暂停
    pub release_frames: u16,  // 连续清白多少帧才恢复
}

impl Default for LoopThresholds {
    fn default() -> Self {
        Self { energy_ratio_db: -6.0, min_xcorr: 0.6, max_lag_frames: 50, hold_frames: 30, release_frames: 50 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopEvidence {
    pub suspected: bool,
    pub lag_frames: u16,
    pub xcorr: f32,
    pub ratio_db: f32,
}

/// 给定一窗注入能量序列与采集能量序列，估计延迟 + 跨相关 + 能量比，判定本窗是否疑似回环。
pub fn detect_loop(injected: &[FrameEnergy], captured: &[FrameEnergy], th: &LoopThresholds) -> LoopEvidence {
    // 取 rms 序列；按 lag 求归一化跨相关峰值；能量比 = 10log10(mean(cap)/mean(inj))。
    // 实现细节：整数转 f32 标量运算，窗口短(≤max_lag+hold)，无堆分配压力。
    // ……（按测试驱动实现：右移 lag 的衰减回声应被命中，独立随机序列应不命中）
    unimplemented!("由 Step 2 测试驱动实现")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopGuardState {
    Clear,
    Suspected { streak: u16 },
    Paused { clear_streak: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardAction { Pause, Resume }

/// 滞回转移：连续 hold_frames 帧疑似 -> Pause；Paused 需连续 release_frames 帧清白 -> Resume。
pub fn step_guard(state: LoopGuardState, ev: &LoopEvidence, th: &LoopThresholds) -> (LoopGuardState, Option<GuardAction>) {
    match state {
        LoopGuardState::Clear => {
            if ev.suspected { (LoopGuardState::Suspected { streak: 1 }, None) }
            else { (LoopGuardState::Clear, None) }
        }
        LoopGuardState::Suspected { streak } => {
            if ev.suspected {
                let s = streak + 1;
                if s >= th.hold_frames { (LoopGuardState::Paused { clear_streak: 0 }, Some(GuardAction::Pause)) }
                else { (LoopGuardState::Suspected { streak: s }, None) }
            } else { (LoopGuardState::Clear, None) }
        }
        LoopGuardState::Paused { clear_streak } => {
            if ev.suspected { (LoopGuardState::Paused { clear_streak: 0 }, None) }
            else {
                let c = clear_streak + 1;
                if c >= th.release_frames { (LoopGuardState::Clear, Some(GuardAction::Resume)) }
                else { (LoopGuardState::Paused { clear_streak: c }, None) }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meter::FrameEnergy;

    fn e(rms: i32) -> FrameEnergy { FrameEnergy { rms_q15: rms, peak: rms as i16, n: 480 } }

    #[test]
    fn detect_loop_flags_delayed_echo() {
        let inj: Vec<FrameEnergy> = (0..64).map(|i| e(((i * 37) % 5000) + 200)).collect();
        // captured = injected 右移 12 帧、-3dB 衰减
        let lag = 12usize;
        let mut cap = vec![e(0); inj.len()];
        for i in lag..inj.len() { cap[i] = e((inj[i - lag].rms_q15 as f32 * 0.708) as i32); }
        let ev = detect_loop(&inj, &cap, &LoopThresholds::default());
        assert!(ev.suspected);
        assert!((11..=13).contains(&ev.lag_frames), "lag={}", ev.lag_frames);
        assert!((ev.ratio_db + 3.0).abs() < 1.5, "ratio_db={}", ev.ratio_db);
    }

    #[test]
    fn detect_loop_ignores_quiet_or_uncorrelated() {
        let inj: Vec<FrameEnergy> = (0..64).map(|i| e(((i * 91) % 6000) + 500)).collect();
        // 远低于注入（ratio_db < -6dB）
        let quiet: Vec<FrameEnergy> = (0..64).map(|_| e(50)).collect();
        assert!(!detect_loop(&inj, &quiet, &LoopThresholds::default()).suspected);
        // 高能量但与注入不相关
        let other: Vec<FrameEnergy> = (0..64).map(|i| e(((i * 13 + 7) % 6000) + 500)).collect();
        assert!(!detect_loop(&inj, &other, &LoopThresholds::default()).suspected);
    }

    #[test]
    fn guard_hysteresis_pause_and_resume() {
        let th = LoopThresholds { hold_frames: 3, release_frames: 3, ..Default::default() };
        let yes = LoopEvidence { suspected: true, lag_frames: 12, xcorr: 0.8, ratio_db: -3.0 };
        let no = LoopEvidence { suspected: false, lag_frames: 0, xcorr: 0.0, ratio_db: -20.0 };
        let mut st = LoopGuardState::Clear;
        let mut acts = Vec::new();
        for _ in 0..3 { let (s, a) = step_guard(st, &yes, &th); st = s; if let Some(a) = a { acts.push(a); } }
        assert_eq!(acts, vec![GuardAction::Pause]);
        // 单帧 Clear 不立即恢复
        let (s, a) = step_guard(st, &no, &th); st = s; assert!(a.is_none());
        // 需连续 release_frames 帧 Clear 才 Resume（已走 1 帧，再走 2 帧）
        for _ in 0..2 { let (s2, a2) = step_guard(st, &no, &th); st = s2; if let Some(a2) = a2 { acts.push(a2); } }
        assert_eq!(acts, vec![GuardAction::Pause, GuardAction::Resume]);
    }
}
```

- [ ] **Step 2: 由测试驱动实现 `detect_loop`**（按 `unimplemented!` 处补：rms 序列 lag 扫描取归一化跨相关峰值，能量比 `10*log10(mean_cap/mean_inj)`；`suspected = xcorr>=min_xcorr && ratio_db>=energy_ratio_db && lag<=max_lag_frames`）。

- [ ] **Step 3: 在 `lib.rs` 导出**

```rust
pub mod loopcheck;
pub use loopcheck::{detect_loop, step_guard, GuardAction, LoopEvidence, LoopGuardState, LoopThresholds};
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p diagnostics`
Expected: meter 1 + isolation 2 + loopcheck 3 = 6 passed。

- [ ] **Step 5: 提交**

```bash
git add crates/diagnostics
git commit -m "feat(diagnostics): 回环检测与滞回状态机（防循环第二道防线）"
```

---

## Task 21: audio-dsp VAD 省成本

**Files:**
- Create: `crates/audio-dsp/src/vad.rs`
- Modify: `crates/audio-dsp/src/lib.rs`

- [ ] **Step 1: 写 `vad.rs`（纯函数 + 滞回状态机 + 测试）**

```rust
//! 能量 + 过零率 VAD：静音/噪声帧不发往 Gemini，省成本。
//! 纯整数运算、零堆分配、无 await/锁；可单测。
pub fn frame_energy_rms(samples: &[i16]) -> u32 {
    if samples.is_empty() { return 0; }
    let acc: i64 = samples.iter().map(|&s| (s as i64) * (s as i64)).sum();
    ((acc / samples.len() as i64) as f64).sqrt() as u32
}

pub fn zero_crossing_rate(samples: &[i16]) -> u16 {
    const DEADZONE: i16 = 64; // 小死区抑制零噪声抖动
    let mut count = 0u16;
    let mut prev_sign: i8 = 0;
    for &s in samples {
        let sign = if s > DEADZONE { 1 } else if s < -DEADZONE { -1 } else { prev_sign };
        if prev_sign != 0 && sign != 0 && sign != prev_sign { count += 1; }
        if sign != 0 { prev_sign = sign; }
    }
    count
}

#[derive(Debug, Clone, Copy)]
pub struct VadConfig {
    pub rms_open: u32,
    pub rms_close: u32,
    pub zcr_noise_max: u16,
    pub hangover_frames: u16,
    pub attack_frames: u16,
}

impl VadConfig {
    pub fn default_uplink() -> Self {
        Self { rms_open: 600, rms_close: 300, zcr_noise_max: 168 /* ≈480*0.35 */, hangover_frames: 30, attack_frames: 2 }
    }
}

/// 瞬时有声候选：currently_speaking 选 open/close 阈值实现幅度滞回；高 ZCR + 低 RMS 判噪声。
pub fn classify_frame(samples: &[i16], cfg: &VadConfig, currently_speaking: bool) -> bool {
    let rms = frame_energy_rms(samples);
    let zcr = zero_crossing_rate(samples);
    let threshold = if currently_speaking { cfg.rms_close } else { cfg.rms_open };
    if rms < threshold { return false; }
    if zcr > cfg.zcr_noise_max && rms < cfg.rms_open { return false; } // 高频低能噪声
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadDecision { Send, Drop }

#[derive(Debug, Clone)]
pub struct Vad {
    cfg: VadConfig,
    speaking: bool,
    attack_count: u16,
    hangover_count: u16,
}

impl Vad {
    pub fn new(cfg: VadConfig) -> Self {
        Self { cfg, speaking: false, attack_count: 0, hangover_count: 0 }
    }
    pub fn is_speaking(&self) -> bool { self.speaking }

    /// 喂一帧（重采样前的原始 in_rate 样本即可），返回 Send/Drop。
    pub fn observe(&mut self, samples: &[i16]) -> VadDecision {
        let voiced = classify_frame(samples, &self.cfg, self.speaking);
        if !self.speaking {
            if voiced {
                self.attack_count += 1;
                if self.attack_count >= self.cfg.attack_frames {
                    self.speaking = true;
                    self.hangover_count = self.cfg.hangover_frames;
                    return VadDecision::Send;
                }
            } else {
                self.attack_count = 0;
            }
            VadDecision::Drop
        } else {
            if voiced {
                self.hangover_count = self.cfg.hangover_frames;
                VadDecision::Send
            } else if self.hangover_count > 0 {
                self.hangover_count -= 1;
                VadDecision::Send // 补尾音防截断
            } else {
                self.speaking = false;
                self.attack_count = 0;
                VadDecision::Drop
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VadStats {
    pub frames_total: u64,
    pub frames_sent: u64,
    pub frames_dropped: u64,
}

impl VadStats {
    pub fn record(&mut self, d: VadDecision) {
        self.frames_total += 1;
        match d { VadDecision::Send => self.frames_sent += 1, VadDecision::Drop => self.frames_dropped += 1 }
    }
    pub fn saved_ratio(&self) -> f64 {
        if self.frames_total == 0 { 0.0 } else { self.frames_dropped as f64 / self.frames_total as f64 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(amp: i16, n: usize) -> Vec<i16> {
        (0..n).map(|i| if i % 2 == 0 { amp } else { -amp }).collect()
    }

    #[test]
    fn silence_frame_is_dropped() {
        let mut vad = Vad::new(VadConfig::default_uplink());
        assert_eq!(vad.observe(&[0i16; 480]), VadDecision::Drop);
        let mut stats = VadStats::default();
        let mut v2 = Vad::new(VadConfig::default_uplink());
        for _ in 0..100 { stats.record(v2.observe(&[0i16; 480])); }
        assert_eq!(stats.frames_sent, 0);
        assert!((stats.saved_ratio() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn loud_speech_frame_is_sent_after_attack() {
        let cfg = VadConfig::default_uplink();
        let mut vad = Vad::new(cfg);
        let loud = tone(8000, 480);
        for _ in 0..cfg.attack_frames { let _ = vad.observe(&loud); }
        assert_eq!(vad.observe(&loud), VadDecision::Send);
        assert!(vad.is_speaking());
    }

    #[test]
    fn hysteresis_prevents_flapping_on_borderline_rms() {
        let cfg = VadConfig::default_uplink();
        let border = tone(450, 480); // close(300) < rms < open(600)
        let mut from_silence = Vad::new(cfg);
        assert_eq!(from_silence.observe(&border), VadDecision::Drop); // 未达 open
        let mut speaking = Vad::new(cfg);
        let loud = tone(8000, 480);
        for _ in 0..(cfg.attack_frames + 1) { let _ = speaking.observe(&loud); }
        assert_eq!(speaking.observe(&border), VadDecision::Send); // 高于 close 不关闭
    }

    #[test]
    fn hangover_keeps_sending_tail_after_speech_stops() {
        let cfg = VadConfig::default_uplink();
        let mut vad = Vad::new(cfg);
        let loud = tone(8000, 480);
        for _ in 0..(cfg.attack_frames + 1) { let _ = vad.observe(&loud); }
        let silence = [0i16; 480];
        for _ in 0..cfg.hangover_frames { assert_eq!(vad.observe(&silence), VadDecision::Send); }
        assert_eq!(vad.observe(&silence), VadDecision::Drop);
        assert!(!vad.is_speaking());
    }

    #[test]
    fn frame_energy_rms_is_pure_and_correct() {
        assert_eq!(frame_energy_rms(&[0i16; 256]), 0);
        assert!((frame_energy_rms(&[3000; 256]) as i32 - 3000).abs() <= 1);
        assert_eq!(frame_energy_rms(&[]), 0);
    }

    #[test]
    fn high_zcr_low_energy_classified_as_noise_not_speech() {
        let cfg = VadConfig::default_uplink();
        let noise = tone(200, 480); // 每样本翻转、低幅
        assert!(zero_crossing_rate(&noise) > cfg.zcr_noise_max);
        assert!(!classify_frame(&noise, &cfg, false));
        let mut vad = Vad::new(cfg);
        assert_eq!(vad.observe(&noise), VadDecision::Drop);
    }
}
```

- [ ] **Step 2: 在 `lib.rs` 导出**

```rust
pub mod vad;
pub use vad::{classify_frame, frame_energy_rms, zero_crossing_rate, Vad, VadConfig, VadDecision, VadStats};
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p audio-dsp`
Expected: resample 2 + vad 6 = 8 passed。

- [ ] **Step 4: 提交**

```bash
git add crates/audio-dsp
git commit -m "feat(audio-dsp): 能量+过零率 VAD 与滞回状态机（省成本）"
```

---

## Task 22: audio-cpal 实现 `watch_devices`（热插拔，唯一平台码处）

**Files:**
- Modify: `crates/audio-cpal/src/lib.rs`

> 这是 M2 唯一允许新增平台差异的地方。cpal 0.15 无原生 hotplug，用轮询-diff（~1s）。**关键保护：`list_with_timeout` 2s 超时返回空列表时不得触发全量 Removed**——空列表视为「枚举暂时失败」，跳过本轮 diff。

- [ ] **Step 1: 实现 `watch_devices`（在 `impl AudioBackend for CpalBackend` 内补方法）**

```rust
    fn watch_devices(&self) -> Result<DeviceWatchHandle, AudioError> {
        use std::sync::mpsc;
        let (tx, rx) = mpsc::channel();
        // 复用 list_*_with_timeout 防枚举卡死；独立后台线程轮询。
        std::thread::Builder::new()
            .name("cpal-device-watch".into())
            .spawn(move || {
                let backend = CpalBackend::new();
                let mut prev: Option<(Vec<DeviceInfo>, Vec<DeviceInfo>)> = None;
                loop {
                    let inputs = backend.list_inputs().unwrap_or_default();
                    let outputs = backend.list_outputs().unwrap_or_default();
                    // 空列表保护：枚举暂时失败（如拔出瞬间 2s 超时）不触发全量 Removed。
                    if inputs.is_empty() && outputs.is_empty() {
                        std::thread::sleep(std::time::Duration::from_millis(1000));
                        continue;
                    }
                    let changed = match &prev {
                        Some((pi, po)) => pi != &inputs || po != &outputs,
                        None => false, // 首轮只建基线，不发事件
                    };
                    if changed {
                        if tx.send(RawDeviceEvent::ListChanged { inputs: inputs.clone(), outputs: outputs.clone() }).is_err() {
                            break; // 接收端已丢弃，结束线程
                        }
                    }
                    prev = Some((inputs, outputs));
                    std::thread::sleep(std::time::Duration::from_millis(1000));
                }
            })
            .map_err(|e| AudioError::OpenStream(e.to_string()))?;
        Ok(DeviceWatchHandle::new(rx))
    }
```

（`use` 段补 `audio_core::{Direction, RawDeviceEvent, DeviceWatchHandle}`；`list_*_with_timeout` 若 M1 已有则复用，否则此处用 `list_inputs/list_outputs`。）

- [ ] **Step 2: 测试（不真正等 1s，仅验证句柄可创建、不 panic）**

```rust
    #[test]
    fn watch_devices_returns_handle_without_panicking() {
        let backend = CpalBackend::new();
        let h = backend.watch_devices().expect("watch handle");
        // 立即 try_recv 应为空（首轮建基线，无事件），不阻塞、不 panic。
        assert!(h.try_recv().is_none());
    }
```

- [ ] **Step 3: 运行测试（双平台）**

Run: `cargo test -p audio-cpal`
Expected: 原有 + watch 测试全 passed（CI 无声卡时句柄仍可创建）。本机 macOS 与 Windows 各跑一次。

- [ ] **Step 4: 跑全 workspace 确认 trait 破坏性扩展已闭合**

Run: `cargo build --workspace`
Expected: 编译成功（Task 14 引入的 trait 缺口此时补齐）。

- [ ] **Step 5: 提交**

```bash
git add crates/audio-cpal
git commit -m "feat(audio-cpal): watch_devices 轮询热插拔（空列表保护，唯一平台码）"
```

---

## Task 23: engine 控制面扩展（Paused 子态 + 防循环事件）

**Files:**
- Modify: `crates/engine/src/control.rs`

- [ ] **Step 1: 在 `control.rs` 扩展 `SessionState` / `ControlEvent`（含测试）**

```rust
// SessionState 新增 Paused（区别于网络 Error，UI 弹「配置修复」而非「网络错误」）
//   Idle, Starting, Running, Reconnecting{attempt}, Paused, Error(String)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseReason { AcousticLoop }

// ControlEvent 新增：
//   LoopSuspected { lag_frames: u16, xcorr: f32 }
//   TranslationPaused { reason: PauseReason }
//   TranslationResumed
```

更新 `worst_state` 的 `rank`：`Paused` rank 介于 `Reconnecting`(3) 与 `Error`(4) 之间——给 `Paused` 排在并列或单独一档，确保暂停态优先于重连显示（便于 UI 弹修复提示）。建议 rank：`Error=5, Paused=4, Reconnecting=3, Starting=2, Running=1, Idle=0`。

```rust
    #[test]
    fn worst_picks_paused_over_reconnecting() {
        let up = SessionState::Paused;
        let down = SessionState::Reconnecting { attempt: 2 };
        assert_eq!(worst_state(&up, &down), SessionState::Paused);
    }

    #[test]
    fn worst_picks_error_over_paused() {
        assert_eq!(
            worst_state(&SessionState::Error("net".into()), &SessionState::Paused),
            SessionState::Error("net".into())
        );
    }
```

> 注：`ControlEvent::LoopSuspected` 含 `f32`，会破坏 `#[derive(Eq)]`——把 `ControlEvent` 的 `Eq` 去掉只留 `PartialEq`（M1 测试用 `assert_eq!` 仍可用 `PartialEq`）。

- [ ] **Step 2: 运行测试**

Run: `cargo test -p engine control`
Expected: M1 原有 3 + 新增 2 = 5 passed。

- [ ] **Step 3: 提交**

```bash
git add crates/engine
git commit -m "feat(engine): 控制面新增 Paused 子态与防循环事件"
```

---

## Task 24: engine 路由矩阵纯函数 `route`

**Files:**
- Modify: `crates/engine/Cargo.toml`
- Create: `crates/engine/src/route.rs`
- Modify: `crates/engine/src/lib.rs`

- [ ] **Step 1: 更新 `crates/engine/Cargo.toml`（保持不引入 audio-cpal）**

```toml
[dependencies]
audio-core = { path = "../audio-core" }
gemini-live = { path = "../gemini-live" }
audio-dsp = { path = "../audio-dsp" }
diagnostics = { path = "../diagnostics" }
device-manager = { path = "../device-manager" }
tokio = { workspace = true }
thiserror.workspace = true

[dev-dependencies]
tokio = { workspace = true, features = ["rt-multi-thread", "macros", "time", "sync"] }
```

- [ ] **Step 2: 写 `route.rs`（纯类型 + build_routes/validate_isolation/active_links + 测试）**

```rust
//! 路由矩阵：启动时一次性生成、运行中只读。平台无关纯函数，用 DeviceId 字符串引用，不碰 cpal。
use audio_core::{DeviceId, DeviceInfo};
use crate::control::{SourceLang, TranslateMode};
use diagnostics::{validate_isolation as diag_isolation, IsolationError, LinkRoute};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind { Uplink, Downlink }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRole {
    pub kind: LinkKind,
    pub target_lang: String,
    pub source: SourceLang,
    pub in_dev: DeviceId,
    pub out_dev: DeviceId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteMatrix {
    pub uplink: Option<LinkRole>,
    pub downlink: Option<LinkRole>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkIntent {
    pub in_dev: DeviceId,
    pub out_dev: DeviceId,
    pub target_lang: String,
    pub source: SourceLang,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSpec {
    pub mode: TranslateMode,
    pub uplink: LinkIntent,
    pub downlink: LinkIntent,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RouteError {
    #[error("设备未找到: {0}")]
    DeviceNotFound(String),
    #[error("物理隔离冲突: 注入设备 {0} 与采集设备相同/重叠，拒绝启动")]
    SourceSinkOverlap(String),
    #[error("输出回流虚拟采集设备 {0}，可能形成自激环路")]
    VirtualLoopback(String),
}

impl From<IsolationError> for RouteError {
    fn from(e: IsolationError) -> Self {
        match e {
            IsolationError::SourceSinkOverlap { device } => RouteError::SourceSinkOverlap(device),
            IsolationError::OutputIsVirtualCaptureSource { device } => RouteError::VirtualLoopback(device),
        }
    }
}

/// 按 mode 投影哪些链路应启（供编排/测试断言「仅上行时 downlink=None」）。
pub fn active_links(mode: TranslateMode) -> (bool, bool) {
    match mode {
        TranslateMode::Bidirectional => (true, true),
        TranslateMode::UplinkOnly => (true, false),
        TranslateMode::DownlinkOnly => (false, true),
    }
}

fn find<'a>(devs: &'a [DeviceInfo], id: &DeviceId) -> Result<&'a DeviceInfo, RouteError> {
    devs.iter().find(|d| &d.id == id).ok_or_else(|| RouteError::DeviceNotFound(id.0.clone()))
}

/// 按 mode 装配链路。downlink 走「听者定目标」：source=Auto、target=听者语言 A。仅查 DeviceInfo 名表，不碰 cpal。
pub fn build_routes(spec: &RouteSpec, devices: &[DeviceInfo]) -> Result<RouteMatrix, RouteError> {
    let (up, down) = active_links(spec.mode);
    let uplink = if up {
        find(devices, &spec.uplink.in_dev)?; find(devices, &spec.uplink.out_dev)?;
        Some(LinkRole { kind: LinkKind::Uplink, target_lang: spec.uplink.target_lang.clone(),
            source: spec.uplink.source.clone(), in_dev: spec.uplink.in_dev.clone(), out_dev: spec.uplink.out_dev.clone() })
    } else { None };
    let downlink = if down {
        find(devices, &spec.downlink.in_dev)?; find(devices, &spec.downlink.out_dev)?;
        Some(LinkRole { kind: LinkKind::Downlink, target_lang: spec.downlink.target_lang.clone(),
            source: SourceLang::Auto, in_dev: spec.downlink.in_dev.clone(), out_dev: spec.downlink.out_dev.clone() })
    } else { None };
    Ok(RouteMatrix { uplink, downlink })
}

/// 第一道结构性闸：委托 diagnostics::validate_isolation（源汇不重叠 + 输出不回流虚拟采集源）。
pub fn validate_isolation(matrix: &RouteMatrix, devices: &[DeviceInfo]) -> Result<(), RouteError> {
    let mut links = Vec::new();
    for role in [matrix.uplink.as_ref(), matrix.downlink.as_ref()].into_iter().flatten() {
        let sv = find(devices, &role.in_dev)?.is_virtual;
        let kv = find(devices, &role.out_dev)?.is_virtual;
        links.push(LinkRoute { source: role.in_dev.clone(), sink: role.out_dev.clone(), source_is_virtual: sv, sink_is_virtual: kv });
    }
    diag_isolation(&links).map_err(RouteError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dev(name: &str, virt: bool) -> DeviceInfo {
        DeviceInfo { id: DeviceId(name.into()), name: name.into(), is_default: false, is_virtual: virt }
    }
    fn devs() -> Vec<DeviceInfo> {
        vec![dev("PhysMic", false), dev("VirtMic", true), dev("VirtSpk", true), dev("PhysHeadset", false)]
    }
    fn intent(i: &str, o: &str, t: &str) -> LinkIntent {
        LinkIntent { in_dev: DeviceId(i.into()), out_dev: DeviceId(o.into()), target_lang: t.into(), source: SourceLang::Auto }
    }
    fn spec(mode: TranslateMode) -> RouteSpec {
        RouteSpec { mode,
            uplink: intent("PhysMic", "VirtMic", "en"),
            downlink: intent("VirtSpk", "PhysHeadset", "zh") }
    }

    #[test]
    fn build_routes_uplink_only_drops_downlink() {
        let m = build_routes(&spec(TranslateMode::UplinkOnly), &devs()).unwrap();
        assert!(m.uplink.is_some() && m.downlink.is_none());
        assert_eq!(active_links(TranslateMode::UplinkOnly), (true, false));
        assert_eq!(active_links(TranslateMode::DownlinkOnly), (false, true));
        assert_eq!(active_links(TranslateMode::Bidirectional), (true, true));
    }

    #[test]
    fn build_routes_bidirectional_lights_two_links() {
        let m = build_routes(&spec(TranslateMode::Bidirectional), &devs()).unwrap();
        let up = m.uplink.unwrap(); let down = m.downlink.unwrap();
        assert_eq!(up.target_lang, "en");
        assert_eq!(down.target_lang, "zh");
        assert_eq!(down.source, SourceLang::Auto); // 听者定目标
        // 四设备互不相等
        assert_ne!(up.in_dev, up.out_dev);
        assert_ne!(up.in_dev, down.in_dev);
        assert_ne!(up.out_dev, down.out_dev);
        assert_ne!(down.in_dev, down.out_dev);
    }

    #[test]
    fn validate_isolation_rejects_source_sink_overlap() {
        // 上行注入设备 == 下行采集设备（都用 VirtMic）
        let mut s = spec(TranslateMode::Bidirectional);
        s.downlink.in_dev = DeviceId("VirtMic".into());
        let m = build_routes(&s, &devs()).unwrap();
        assert!(matches!(validate_isolation(&m, &devs()), Err(RouteError::VirtualLoopback(_)) | Err(RouteError::SourceSinkOverlap(_))));
        // 对照组：合法四设备
        let ok = build_routes(&spec(TranslateMode::Bidirectional), &devs()).unwrap();
        assert_eq!(validate_isolation(&ok, &devs()), Ok(()));
    }

    #[test]
    fn validate_isolation_rejects_output_to_virtual_capture() {
        // 上行 out == 下行 in == VirtSpk（虚拟采集源）
        let mut s = spec(TranslateMode::Bidirectional);
        s.uplink.out_dev = DeviceId("VirtSpk".into());
        let m = build_routes(&s, &devs()).unwrap();
        assert!(matches!(validate_isolation(&m, &devs()), Err(RouteError::VirtualLoopback(_))));
    }

    #[test]
    fn build_routes_missing_device_errs() {
        let mut s = spec(TranslateMode::Bidirectional);
        s.uplink.in_dev = DeviceId("Ghost".into());
        assert!(matches!(build_routes(&s, &devs()), Err(RouteError::DeviceNotFound(_))));
    }
}
```

- [ ] **Step 3: 在 `lib.rs` 挂模块**

```rust
pub mod route;
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p engine route`
Expected: 5 passed。

- [ ] **Step 5: 提交**

```bash
git add crates/engine
git commit -m "feat(engine): 路由矩阵纯函数（三态装配 + 物理隔离校验）"
```

---

## Task 25: engine 单链路运行单元 `link` + 编排器 `orchestrator`

**Files:**
- Create: `crates/engine/src/link.rs`
- Create: `crates/engine/src/orchestrator.rs`
- Modify: `crates/engine/src/lib.rs`

> `link.rs` 的真实 `spawn_link`（open_input/output + connect_with_retry + 上下行 pump）涉及真实音频/网络，难以纯单测——其装配逻辑由真机覆盖（Task 26）。**自动可单测的核心是 orchestrator 的状态投影**：用内存 `watch` 通道 + 内存 mpsc 验证「两链路独立上报 + worst_state 投影」，不连真网络。

- [ ] **Step 1: 写 `link.rs`（LinkHandle + spawn_link 骨架；状态上报用 watch）**

```rust
//! 单条链路的独立运行单元：独立 Session、独立重连、独立背压、独立过期帧丢弃。
use std::sync::Arc;
use audio_core::AudioBackend;
use tokio::sync::watch;
use tokio::task::AbortHandle;
use crate::control::SessionState;
use crate::route::{LinkKind, LinkRole, RouteError};

pub struct LinkHandle {
    pub kind: LinkKind,
    pub state: watch::Receiver<SessionState>,
    abort: AbortHandle,
}

impl LinkHandle {
    pub fn current_state(&self) -> SessionState { self.state.borrow().clone() }
    pub fn abort(&self) { self.abort.abort(); }
}

/// 启动一条链路：open_input/open_output → connect_with_retry → spawn 上/下行 pump。
/// 上行：pop_slice 满 chunk → (VAD 门控) → resample 16k → Session.send。
/// 下行：Session.recv → 切块 resample → out_rate → push_slice。SessionState 经 watch::Sender 上报。
pub async fn spawn_link(
    role: &LinkRole,
    backend: Arc<dyn AudioBackend>,
    make_url: Arc<dyn Fn() -> String + Send + Sync>,
) -> Result<LinkHandle, RouteError> {
    // 真实接线（音频流 + Session）；本骨架的网络/音频部分由真机联调覆盖（Task 26）。
    let _ = (role, backend, make_url);
    unimplemented!("真实音频/网络接线，由真机联调验证；单测覆盖 orchestrator 状态投影")
}
```

- [ ] **Step 2: 写 `orchestrator.rs`（持 0..2 LinkHandle + worst_state 投影 + 内存测试）**

```rust
//! 顶层编排：持 RouteMatrix 与 0..2 LinkHandle，聚合两 watch，子状态变更即 worst_state 投影并发事件。
use std::sync::Arc;
use audio_core::AudioBackend;
use tokio::sync::{mpsc, watch};
use crate::control::{worst_state, ControlEvent, SessionState};
use crate::link::{spawn_link, LinkHandle};
use crate::route::{LinkKind, RouteError, RouteMatrix};

pub struct Orchestrator {
    matrix: RouteMatrix,
    uplink: Option<LinkHandle>,
    downlink: Option<LinkHandle>,
}

impl Orchestrator {
    pub fn top_state(&self) -> SessionState {
        let up = self.uplink.as_ref().map(|h| h.current_state()).unwrap_or(SessionState::Idle);
        let down = self.downlink.as_ref().map(|h| h.current_state()).unwrap_or(SessionState::Idle);
        worst_state(&up, &down)
    }
    pub fn stop(self) {
        if let Some(h) = &self.uplink { h.abort(); }
        if let Some(h) = &self.downlink { h.abort(); }
    }
}

pub async fn start(
    matrix: RouteMatrix,
    backend: Arc<dyn AudioBackend>,
    make_url: Arc<dyn Fn() -> String + Send + Sync>,
    evt_tx: mpsc::Sender<ControlEvent>,
) -> Result<Orchestrator, RouteError> {
    let mut uplink = None;
    let mut downlink = None;
    if let Some(role) = &matrix.uplink { uplink = Some(spawn_link(role, backend.clone(), make_url.clone()).await?); }
    if let Some(role) = &matrix.downlink { downlink = Some(spawn_link(role, backend.clone(), make_url.clone()).await?); }
    spawn_state_relay(&uplink, &downlink, evt_tx);
    Ok(Orchestrator { matrix, uplink, downlink })
}

/// 聚合两条 link 的 watch：任一变更即发对应 Uplink/DownlinkState 事件（独立上报，互不拖累）。
fn spawn_state_relay(up: &Option<LinkHandle>, down: &Option<LinkHandle>, evt_tx: mpsc::Sender<ControlEvent>) {
    if let Some(h) = up {
        let mut rx = h.state.clone();
        let tx = evt_tx.clone();
        tokio::spawn(async move {
            loop {
                let s = rx.borrow().clone();
                if tx.send(ControlEvent::UplinkState(s)).await.is_err() { break; }
                if rx.changed().await.is_err() { break; }
            }
        });
    }
    if let Some(h) = down {
        let mut rx = h.state.clone();
        let tx = evt_tx.clone();
        tokio::spawn(async move {
            loop {
                let s = rx.borrow().clone();
                if tx.send(ControlEvent::DownlinkState(s)).await.is_err() { break; }
                if rx.changed().await.is_err() { break; }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::{LinkKind, LinkRole};
    use audio_core::DeviceId;

    // 测试构造器：用内存 watch 直接造 LinkHandle，绕过真实音频/网络。
    fn handle(kind: LinkKind, init: SessionState) -> (LinkHandle, watch::Sender<SessionState>) {
        let (tx, rx) = watch::channel(init);
        // 用一个永不结束的 dummy task 取 AbortHandle
        let abort = tokio::spawn(async { std::future::pending::<()>().await }).abort_handle();
        (LinkHandle { kind, state: rx, abort }, tx)
    }
    fn role(kind: LinkKind) -> LinkRole {
        LinkRole { kind, target_lang: "en".into(), source: crate::control::SourceLang::Auto,
            in_dev: DeviceId("a".into()), out_dev: DeviceId("b".into()) }
    }

    #[tokio::test]
    async fn orchestrator_projects_worst_state_per_link() {
        let (up_h, _up_tx) = handle(LinkKind::Uplink, SessionState::Running);
        let (down_h, _down_tx) = handle(LinkKind::Downlink, SessionState::Reconnecting { attempt: 2 });
        let (evt_tx, mut evt_rx) = mpsc::channel(8);
        let matrix = RouteMatrix { uplink: Some(role(LinkKind::Uplink)), downlink: Some(role(LinkKind::Downlink)) };
        let orch = Orchestrator { matrix, uplink: Some(up_h), downlink: Some(down_h) };
        // top_state 复用 worst_state，不重新实现
        assert_eq!(orch.top_state(), worst_state(&SessionState::Running, &SessionState::Reconnecting { attempt: 2 }));
        // 手动触发一次 relay（用内部函数；或直接断言 top_state 投影）
        super::spawn_state_relay(&orch.uplink, &orch.downlink, evt_tx);
        let mut got = Vec::new();
        for _ in 0..2 {
            if let Ok(Some(e)) = tokio::time::timeout(std::time::Duration::from_millis(500), evt_rx.recv()).await { got.push(e); }
        }
        // 两条独立事件：Uplink(Running) 与 Downlink(Reconnecting{2})，一条重连不把另一条拖成全停
        assert!(got.contains(&ControlEvent::UplinkState(SessionState::Running)));
        assert!(got.contains(&ControlEvent::DownlinkState(SessionState::Reconnecting { attempt: 2 })));
    }
}
```

- [ ] **Step 3: 在 `lib.rs` 挂模块**

```rust
pub mod link;
pub mod orchestrator;
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p engine`
Expected: control 5 + route 5 + orchestrator 1 = 11 passed。（`spawn_link` 的 `unimplemented!` 不被单测触达。）

- [ ] **Step 5: 提交**

```bash
git add crates/engine
git commit -m "feat(engine): 双链路独立运行单元与编排器（worst_state 投影发事件）"
```

---

## Task 26: CLI 升级为 engine 薄壳 + 真机联调（手动验证）

**Files:**
- Modify: `crates/cli/Cargo.toml`
- Modify: `crates/cli/src/main.rs`
- Create: `docs/superpowers/notes/M2-validation.md`（记录结果）

> CLI 的纯函数部分（`parse_mode`/`build_spec`）可单测；真机双向 30 分钟 + 防循环触发 + 热插拔属手动验收，需真实 GEMINI_API_KEY + BlackHole/VB-CABLE + Zoom/Teams，不可自动化、不得伪造。

- [ ] **Step 1: 更新 `crates/cli/Cargo.toml`**

```toml
[dependencies]
audio-core = { path = "../audio-core" }
audio-cpal = { path = "../audio-cpal" }
audio-dsp = { path = "../audio-dsp" }
gemini-live = { path = "../gemini-live" }
engine = { path = "../engine" }
device-manager = { path = "../device-manager" }
tokio = { workspace = true, features = ["rt-multi-thread", "macros", "time", "signal"] }
anyhow.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
```

- [ ] **Step 2: 重写 `main.rs` 为薄壳**（仅此处 import audio-cpal）：
  - 解析 `--mode <bidirectional|uplink-only|downlink-only>`、`--uplink-in/--uplink-out/--uplink-target`、`--downlink-in/--downlink-out/--downlink-target`。
  - 无 `--uplink-in` 时：`DeviceManager::new(CpalBackend::new())`，打印分类设备表（`[default]`/`[virtual-mic]`/`[virtual-speaker]`/`[physical]`）后退出。
  - 构建 `RouteSpec` → `engine::route::build_routes` → `validate_isolation`；失败打印隔离错误并退出（非 0）。
  - 成功则 `Orchestrator::start(matrix, Arc::new(CpalBackend::new()), make_url, evt_tx)`；起一个 task 把 `ControlEvent` 打到 stdout。
  - `Ctrl+C` → `orch.stop()`。
  - `parse_mode`/`build_spec` 提供纯函数单测。

- [ ] **Step 3: 纯函数单测（加在 main.rs 测试段）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use engine::control::TranslateMode;

    #[test]
    fn parse_mode_defaults_and_variants() {
        assert_eq!(parse_mode(&["--mode".into(), "uplink-only".into()]), TranslateMode::UplinkOnly);
        assert_eq!(parse_mode(&["--mode".into(), "downlink-only".into()]), TranslateMode::DownlinkOnly);
        assert_eq!(parse_mode(&[]), TranslateMode::Bidirectional); // 默认双向
    }

    #[test]
    fn build_spec_maps_args_to_intents() {
        let args: Vec<String> = "--mode bidirectional --uplink-in PhysMic --uplink-out VirtMic --uplink-target en --downlink-in VirtSpk --downlink-out PhysHeadset --downlink-target zh"
            .split(' ').map(String::from).collect();
        let spec = build_spec(&args).unwrap();
        assert_eq!(spec.uplink.in_dev.0, "PhysMic");
        assert_eq!(spec.downlink.out_dev.0, "PhysHeadset");
        assert_eq!(spec.downlink.target_lang, "zh");
    }
}
```

- [ ] **Step 4: 编译 + 列设备冒烟 + 全量自测**

Run:
```bash
cargo build -p cli
cargo run -p cli                 # 打印分类设备表（[default]/[virtual-mic]/[virtual-speaker]）
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
Expected: 编译成功；设备表含分类标签；fmt/clippy 全绿；`cargo test --workspace` 全绿（M0/M1 原有 + M2 新增约 33 个）。

- [ ] **Step 5: 真机联调（手动，需 BlackHole/VB-CABLE + Zoom/Teams + GEMINI_API_KEY）**
  1. 把 Zoom/Teams 麦克风设为虚拟麦克风、扬声器设为虚拟扬声器。
  2. `--mode bidirectional`：本地说中文→对方听英文（上行写入虚拟麦克风）；对方说英文→本地耳机听中文（下行采集虚拟扬声器→下行 Session→物理耳机）。
  3. 连续 30 分钟：不崩、无明显循环/回声；一条链路临时断网时另一条仍 Running 不全停。
  4. `--mode uplink-only` / `downlink-only`：观测对应方向 Gemini 连接未建立（省成本）。
  5. 故意构成声学环路：回环检测数秒内置 `Paused` 并 CLI 打印「配置修复」提示（不依赖 AEC）。
  6. 物理隔离：把上行注入设备与下行采集设备配成同一设备 → CLI 拒绝启动并打印隔离错误。
  7. VAD：本地静默/轻噪时上行停发（`VadStats.saved_ratio` 上升）；说话即恢复。
  8. 防双声：物理耳机只听到译文、听不到远端英文原声。
  9. 热插拔：运行中拔出物理麦 → 数秒内 DeviceLost → Error 子态提示不可用不崩溃；插回后设备列表可刷新。

- [ ] **Step 6: 写验收记录 `docs/superpowers/notes/M2-validation.md`**

```markdown
# M2 验收记录
- 日期 / 平台（macOS / Windows）/ 虚拟设备（BlackHole / VB-CABLE）
- 模式：bidirectional / uplink-only / downlink-only 各自结果
- 双向 30 分钟：崩溃次数 / 明显循环回声 / 单链路断线另一条是否继续
- 防循环：物理隔离拒绝启动；回环检测触发暂停的耗时与「配置修复」提示
- VAD：静默期 saved_ratio、说话恢复及时性、阈值标定（rms_open/close/hangover）
- 防双声：物理耳机是否只听译文
- 热插拔：拔出物理麦 DeviceLost→Error 子态、插回刷新
- 与 PRD 14.1 / 设计 §7 M2 验收的差距
```

- [ ] **Step 7: 提交**

```bash
git add crates/cli docs/superpowers/notes/M2-validation.md
git commit -m "feat(cli): 升级为 engine 编排薄壳（双链路/三态/分类设备表）+ M2 联调记录"
```

**M2 完成检查点（验收标准）：** `cargo test --workspace` 全绿；`cargo clippy --workspace --all-targets -- -D warnings` 无警告；`cargo fmt --all -- --check` 通过；`engine`/`device-manager`/`diagnostics`/`cli` 无 `#[cfg(target_os)]`（仅 audio-cpal 有）；真机双向 30 分钟、防循环、VAD、热插拔项已记录于 M2-validation.md（标注需人工）。

---

## 自查（Self-Review）

### 自动可验收（CI / cargo test，无需真机）
- 设备分类（方向+虚拟名）→ Task 15 `classify` ✅
- 快照 diff（Added/Removed/DefaultChanged）+ DeviceLost 投影 → Task 16 ✅
- DeviceManager 不可变刷新 + 原始事件经纯函数到语义事件（无 cpal）→ Task 17 ✅
- 逐帧能量摘要零分配 → Task 18 ✅
- 物理隔离校验（源汇重叠 / 输出回流虚拟采集源）→ Task 19 + Task 24 `validate_isolation` ✅
- 回环检测（延迟回声命中 / 安静或不相关不命中）+ 滞回 Pause/Resume → Task 20 ✅
- VAD（静音丢弃 / attack / 幅度滞回 / hangover 补尾 / 高 ZCR 低能判噪声 / saved_ratio）→ Task 21 ✅
- watch_devices 句柄可创建不 panic（CI 无声卡）→ Task 22 ✅
- 控制面 Paused 子态投影优先级 → Task 23 ✅
- 路由矩阵三态装配（仅上行 downlink=None / 双向两链路 / 听者定目标 source=Auto / 缺设备干净失败）→ Task 24 ✅
- 编排器 worst_state 投影 + 两链路独立上报（内存 watch/channel）→ Task 25 ✅
- CLI `parse_mode`/`build_spec` 纯函数 → Task 26 ✅
- 双平台编译 + clippy + fmt + 架构纪律（engine 及以上无 cfg）→ Task 22/26 检查点 ✅

### 需真机验收（BlackHole/VB-CABLE + Zoom/Teams + 真实 GEMINI_API_KEY，列入 M2-validation.md）
- 中英双向 30 分钟不崩、无明显循环回声（设计 §7 M2 / PRD 14.1）。
- 两链路独立性：一条断网另一条仍 Running 自动重连。
- 三态省成本：仅上行/仅下行时对应方向 Gemini 连接未建立（抓连接观测）。
- 防循环第二道防线：真实声学环路下数秒内 Paused + 「配置修复」提示，不依赖 AEC；30 分钟正常对话误暂停 0 次（阈值标定）。
- 防双声主观验收：物理耳机只听译文、不出现远端原声。
- VAD 真机阈值标定与句首/句尾不截断、噪声不误触发。
- 热插拔时序：拔出物理麦数秒内 DeviceLost→Error 子态不崩溃，插回刷新；空列表保护避免误报全量 Removed。
- 虚拟设备真实命名是否被 `is_virtual_device_name` 命中；漏判时靠结构性隔离（SourceSinkOverlap）兜底。

### 跨子系统依赖说明（集成阶段验证）
- `engine::link::spawn_link` 的干净中止依赖 `gemini-live::connect` 提供 JoinHandle/取消通道。当前 connect 内部 spawn 的两个 task 仅靠 drop sender 间接停下行 read task。M2 的 `AbortHandle` 可中止 link 外层 pump task，但 Session 内部 task 的彻底回收需 gemini-live 句柄改造（可作为本切片内的小补强 Task，或 M2 集成期真机长跑确认无 task 泄漏）。若需补强，在 Task 25 前插入「gemini-live Session 返回 abort 句柄」的微改造并补一条 mock WS 取消测试。

### 占位符扫描
- `link.rs::spawn_link` 与 `loopcheck.rs::detect_loop` 的 `unimplemented!` 均为**测试驱动的待实现体**（detect_loop 由 Task 20 Step 2 测试驱动补全；spawn_link 由真机覆盖，单测不触达），非遗留占位。交付前 `detect_loop` 必须实现到测试绿；`spawn_link` 必须实现真实音频/网络接线（真机验证），不得在交付物里留 `unimplemented!`。

### 类型一致性
`DeviceUse`/`Direction`/`DeviceSnapshot`/`DeviceEvent`/`RawDeviceEvent`/`DeviceWatchHandle`/`FrameEnergy`/`LinkRoute`/`IsolationError`/`LoopThresholds`/`LoopEvidence`/`LoopGuardState`/`GuardAction`/`Vad`/`VadConfig`/`VadDecision`/`VadStats`/`LinkRole`/`RouteMatrix`/`RouteSpec`/`RouteError`/`LinkHandle`/`Orchestrator` 在定义与跨 crate 调用处签名一致；`engine::route::RouteError` 经 `From<IsolationError>` 与 diagnostics 衔接；`control::worst_state` 复用不重写。