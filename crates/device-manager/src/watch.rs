//! 设备语义事件与「正在使用却消失」投影。
use crate::snapshot::DeviceSnapshot;
use audio_core::{DeviceId, Direction};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceEvent {
    Added { id: DeviceId, dir: Direction },
    Removed { id: DeviceId, dir: Direction },
    DefaultChanged { dir: Direction, new: DeviceId },
    DeviceLost { id: DeviceId, dir: Direction },
}

/// 正在使用的设备在新快照中缺失 -> DeviceLost。
pub fn project_lost(in_use: &[(DeviceId, Direction)], next: &DeviceSnapshot) -> Vec<DeviceEvent> {
    in_use
        .iter()
        .filter(|(id, dir)| !next.contains(id, *dir))
        .map(|(id, dir)| DeviceEvent::DeviceLost {
            id: id.clone(),
            dir: *dir,
        })
        .collect()
}
