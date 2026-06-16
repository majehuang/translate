//! 平台无关音频后端接口。所有平台差异收敛在实现 crate 内，
//! 上层（engine 及以上）禁止出现 #[cfg(target_os)]。

use crate::ring::{AudioConsumer, AudioProducer};
use std::sync::mpsc;
use std::time::Duration;

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
    rx: mpsc::Receiver<RawDeviceEvent>,
}

impl DeviceWatchHandle {
    pub fn new(rx: mpsc::Receiver<RawDeviceEvent>) -> Self {
        Self { rx }
    }

    pub fn try_recv(&self) -> Option<RawDeviceEvent> {
        self.rx.try_recv().ok()
    }

    pub fn recv_timeout(&self, duration: Duration) -> Option<RawDeviceEvent> {
        self.rx.recv_timeout(duration).ok()
    }
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
    /// 订阅设备增删变更。返回低频控制事件通道句柄。
    fn watch_devices(&self) -> Result<DeviceWatchHandle, AudioError>;
}

/// 名字是否匹配已知虚拟音频设备。
pub fn is_virtual_device_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("blackhole")
        || n.contains("vb-audio")
        || n.contains("cable")
        || n.contains("voicemeeter")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_virtual_devices_by_name() {
        assert!(is_virtual_device_name("BlackHole 2ch"));
        assert!(is_virtual_device_name(
            "CABLE Output (VB-Audio Virtual Cable)"
        ));
        assert!(!is_virtual_device_name("MacBook Pro Microphone"));
        assert!(!is_virtual_device_name("Realtek High Definition Audio"));
    }

    #[test]
    fn watch_handle_relays_raw_event() {
        let (tx, rx) = std::sync::mpsc::channel();
        let h = DeviceWatchHandle::new(rx);
        assert!(h.try_recv().is_none());
        tx.send(RawDeviceEvent::ListChanged {
            inputs: vec![],
            outputs: vec![],
        })
        .unwrap();
        assert!(matches!(
            h.try_recv(),
            Some(RawDeviceEvent::ListChanged { .. })
        ));
    }
}
