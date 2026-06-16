//! 诊断：物理隔离校验（第一道防线）+ 回环检测自动暂停（第二道防线）+ 能量摘要。
//! 平台无关、纯函数、热路径零分配。仅依赖 audio-core，零 cpal、零平台条件编译。
pub mod isolation;
pub mod loopcheck;
pub mod meter;
pub use isolation::{validate_isolation, IsolationError, LinkRoute};
pub use loopcheck::{
    detect_loop, step_guard, GuardAction, LoopEvidence, LoopGuardState, LoopThresholds,
};
pub use meter::{frame_energy, FrameEnergy};
