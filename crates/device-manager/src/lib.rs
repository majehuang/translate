//! 平台无关设备管理：枚举/分类/快照 diff/热插拔投影。仅依赖 audio-core trait，零 cpal。
pub mod classify;
pub mod manager;
pub mod snapshot;
pub mod watch;
pub use audio_core::Direction;
pub use classify::{classify, DeviceUse};
pub use manager::DeviceManager;
pub use snapshot::{diff_snapshots, DeviceSnapshot};
pub use watch::{project_lost, DeviceEvent};
