//! 音频信号处理：重采样（后续扩展 VAD/降噪/AEC）。
pub mod resample;
pub use resample::Resampler;
