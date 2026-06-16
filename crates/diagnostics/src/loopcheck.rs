//! 第二道防线：回环检测 + 滞回状态机。纯函数，无状态/无 IO。
use crate::meter::FrameEnergy;

#[derive(Debug, Clone, Copy)]
pub struct LoopThresholds {
    pub energy_ratio_db: f32,
    pub min_xcorr: f32,
    pub max_lag_frames: u16,
    pub hold_frames: u16,
    pub release_frames: u16,
}

impl Default for LoopThresholds {
    fn default() -> Self {
        Self {
            energy_ratio_db: -6.0,
            min_xcorr: 0.6,
            max_lag_frames: 50,
            hold_frames: 30,
            release_frames: 50,
        }
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
pub fn detect_loop(
    injected: &[FrameEnergy],
    captured: &[FrameEnergy],
    th: &LoopThresholds,
) -> LoopEvidence {
    if injected.is_empty() || captured.is_empty() {
        return LoopEvidence {
            suspected: false,
            lag_frames: 0,
            xcorr: 0.0,
            ratio_db: f32::NEG_INFINITY,
        };
    }

    if mean_power(injected) <= f64::EPSILON || mean_power(captured) <= f64::EPSILON {
        return LoopEvidence {
            suspected: false,
            lag_frames: 0,
            xcorr: 0.0,
            ratio_db: f32::NEG_INFINITY,
        };
    }
    let max_lag = usize::from(th.max_lag_frames)
        .min(injected.len().saturating_sub(1))
        .min(captured.len().saturating_sub(1));
    let mut best_lag = 0u16;
    let mut best_corr = 0.0f32;

    for lag in 0..=max_lag {
        let pairs = injected.len().min(captured.len().saturating_sub(lag));
        if pairs < 2 {
            continue;
        }
        let corr = lagged_corr(injected, captured, lag, pairs);
        if corr > best_corr {
            best_corr = corr;
            best_lag = lag as u16;
        }
    }

    let ratio_db = aligned_ratio_db(injected, captured, usize::from(best_lag));
    let suspected = best_corr >= th.min_xcorr
        && ratio_db >= th.energy_ratio_db
        && best_lag <= th.max_lag_frames;
    LoopEvidence {
        suspected,
        lag_frames: best_lag,
        xcorr: best_corr,
        ratio_db,
    }
}

fn aligned_ratio_db(injected: &[FrameEnergy], captured: &[FrameEnergy], lag: usize) -> f32 {
    let pairs = injected.len().min(captured.len().saturating_sub(lag));
    if pairs == 0 {
        return f32::NEG_INFINITY;
    }
    let mut injected_power = 0.0f64;
    let mut captured_power = 0.0f64;
    for idx in 0..pairs {
        let i = f64::from(injected[idx].rms_q15.max(0));
        let c = f64::from(captured[idx + lag].rms_q15.max(0));
        injected_power += i * i;
        captured_power += c * c;
    }
    if injected_power <= f64::EPSILON || captured_power <= f64::EPSILON {
        return f32::NEG_INFINITY;
    }
    (10.0 * (captured_power / injected_power).log10()) as f32
}

fn mean_power(frames: &[FrameEnergy]) -> f64 {
    let sum: f64 = frames
        .iter()
        .map(|frame| {
            let rms = f64::from(frame.rms_q15.max(0));
            rms * rms
        })
        .sum();
    sum / frames.len() as f64
}

fn lagged_corr(
    injected: &[FrameEnergy],
    captured: &[FrameEnergy],
    lag: usize,
    pairs: usize,
) -> f32 {
    let mut sum_i = 0.0f64;
    let mut sum_c = 0.0f64;
    for idx in 0..pairs {
        sum_i += f64::from(injected[idx].rms_q15);
        sum_c += f64::from(captured[idx + lag].rms_q15);
    }
    let mean_i = sum_i / pairs as f64;
    let mean_c = sum_c / pairs as f64;

    let mut cov = 0.0f64;
    let mut var_i = 0.0f64;
    let mut var_c = 0.0f64;
    for idx in 0..pairs {
        let di = f64::from(injected[idx].rms_q15) - mean_i;
        let dc = f64::from(captured[idx + lag].rms_q15) - mean_c;
        cov += di * dc;
        var_i += di * di;
        var_c += dc * dc;
    }
    if var_i <= f64::EPSILON || var_c <= f64::EPSILON {
        return 0.0;
    }
    (cov / (var_i.sqrt() * var_c.sqrt())) as f32
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopGuardState {
    Clear,
    Suspected { streak: u16 },
    Paused { clear_streak: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardAction {
    Pause,
    Resume,
}

/// 滞回转移：连续 hold_frames 帧疑似 -> Pause；Paused 需连续 release_frames 帧清白 -> Resume。
pub fn step_guard(
    state: LoopGuardState,
    ev: &LoopEvidence,
    th: &LoopThresholds,
) -> (LoopGuardState, Option<GuardAction>) {
    match state {
        LoopGuardState::Clear => {
            if ev.suspected {
                (LoopGuardState::Suspected { streak: 1 }, None)
            } else {
                (LoopGuardState::Clear, None)
            }
        }
        LoopGuardState::Suspected { streak } => {
            if ev.suspected {
                let next = streak + 1;
                if next >= th.hold_frames {
                    (
                        LoopGuardState::Paused { clear_streak: 0 },
                        Some(GuardAction::Pause),
                    )
                } else {
                    (LoopGuardState::Suspected { streak: next }, None)
                }
            } else {
                (LoopGuardState::Clear, None)
            }
        }
        LoopGuardState::Paused { clear_streak } => {
            if ev.suspected {
                (LoopGuardState::Paused { clear_streak: 0 }, None)
            } else {
                let next = clear_streak + 1;
                if next >= th.release_frames {
                    (LoopGuardState::Clear, Some(GuardAction::Resume))
                } else {
                    (LoopGuardState::Paused { clear_streak: next }, None)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(rms: i32) -> FrameEnergy {
        FrameEnergy {
            rms_q15: rms,
            peak: rms as i16,
            n: 480,
        }
    }

    #[test]
    fn detect_loop_flags_delayed_echo() {
        let inj: Vec<FrameEnergy> = (0..64).map(|i| e(((i * 37) % 5000) + 200)).collect();
        let lag = 12usize;
        let mut cap = vec![e(0); inj.len()];
        for i in lag..inj.len() {
            cap[i] = e((inj[i - lag].rms_q15 as f32 * 0.708) as i32);
        }
        let ev = detect_loop(&inj, &cap, &LoopThresholds::default());
        assert!(ev.suspected);
        assert!((11..=13).contains(&ev.lag_frames), "lag={}", ev.lag_frames);
        assert!((ev.ratio_db + 3.0).abs() < 1.5, "ratio_db={}", ev.ratio_db);
    }

    #[test]
    fn detect_loop_ignores_quiet_or_uncorrelated() {
        let inj: Vec<FrameEnergy> = (0..64).map(|i| e(((i * 91) % 6000) + 500)).collect();
        let quiet: Vec<FrameEnergy> = (0..64).map(|_| e(50)).collect();
        assert!(!detect_loop(&inj, &quiet, &LoopThresholds::default()).suspected);

        let other: Vec<FrameEnergy> = (0..64).map(|i| e(((i * 13 + 7) % 6000) + 500)).collect();
        assert!(!detect_loop(&inj, &other, &LoopThresholds::default()).suspected);
    }

    #[test]
    fn guard_hysteresis_pause_and_resume() {
        let th = LoopThresholds {
            hold_frames: 3,
            release_frames: 3,
            ..Default::default()
        };
        let yes = LoopEvidence {
            suspected: true,
            lag_frames: 12,
            xcorr: 0.8,
            ratio_db: -3.0,
        };
        let no = LoopEvidence {
            suspected: false,
            lag_frames: 0,
            xcorr: 0.0,
            ratio_db: -20.0,
        };
        let mut state = LoopGuardState::Clear;
        let mut actions = Vec::new();
        for _ in 0..3 {
            let (next, action) = step_guard(state, &yes, &th);
            state = next;
            if let Some(action) = action {
                actions.push(action);
            }
        }
        assert_eq!(actions, vec![GuardAction::Pause]);

        let (next, action) = step_guard(state, &no, &th);
        state = next;
        assert!(action.is_none());
        for _ in 0..2 {
            let (next, action) = step_guard(state, &no, &th);
            state = next;
            if let Some(action) = action {
                actions.push(action);
            }
        }
        assert_eq!(actions, vec![GuardAction::Pause, GuardAction::Resume]);
    }
}
