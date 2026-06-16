//! 平台无关音频核心类型与接口。
pub mod backend;
pub mod frame;
pub mod ring;
pub use backend::{
    is_virtual_device_name, AudioBackend, AudioError, DeviceId, DeviceInfo, DeviceWatchHandle,
    Direction, InputStream, OutputStream, RawDeviceEvent, StreamCfg,
};
pub use frame::PcmFrame;
pub use ring::{audio_channel, AudioConsumer, AudioProducer};
