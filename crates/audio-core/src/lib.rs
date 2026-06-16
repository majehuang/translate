//! 平台无关音频核心类型与接口。
pub mod frame;
pub mod ring;
pub use frame::PcmFrame;
pub use ring::{audio_channel, AudioConsumer, AudioProducer};
