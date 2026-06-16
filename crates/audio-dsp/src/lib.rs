//! 音频信号处理：重采样（后续扩展 VAD/降噪/AEC）。
pub mod resample;
pub mod vad;
pub use resample::Resampler;
pub use vad::{
    classify_frame, frame_energy_rms, zero_crossing_rate, Vad, VadConfig, VadDecision, VadStats,
};
