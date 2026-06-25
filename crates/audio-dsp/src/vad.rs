//! 能量 + 过零率 VAD：静音/噪声帧不发往 Gemini，省成本。
//! 纯整数运算、零堆分配、无 await/锁；可单测。
pub fn frame_energy_rms(samples: &[i16]) -> u32 {
    if samples.is_empty() {
        return 0;
    }
    let acc: i64 = samples
        .iter()
        .map(|&sample| (sample as i64) * (sample as i64))
        .sum();
    ((acc / samples.len() as i64) as f64).sqrt() as u32
}

pub fn zero_crossing_rate(samples: &[i16]) -> u16 {
    const DEADZONE: i16 = 64;
    let mut count = 0u16;
    let mut prev_sign = 0i8;
    for &sample in samples {
        let sign = if sample > DEADZONE {
            1
        } else if sample < -DEADZONE {
            -1
        } else {
            prev_sign
        };
        if prev_sign != 0 && sign != 0 && sign != prev_sign {
            count += 1;
        }
        if sign != 0 {
            prev_sign = sign;
        }
    }
    count
}

#[derive(Debug, Clone, Copy)]
pub struct VadConfig {
    /// 开启语音的绝对 RMS 下限（噪声底之上还需至少到这个值）。
    pub rms_open: u32,
    /// 维持语音的 RMS（滞回，低于 open 以免抖动）。
    pub rms_close: u32,
    pub zcr_noise_max: u16,
    pub hangover_frames: u16,
    pub attack_frames: u16,
    /// 开启阈值相对自适应噪声底的倍数（百分比）。300 = 需达噪声底的 3 倍。
    /// 噪声越大，动态门限自动抬高，抑制把环境噪音当语音。
    pub noise_margin_pct: u32,
}

impl VadConfig {
    pub fn default_uplink() -> Self {
        Self {
            rms_open: 1000,
            rms_close: 550,
            zcr_noise_max: 168,
            hangover_frames: 30,
            attack_frames: 4,
            noise_margin_pct: 250,
        }
    }
}

pub fn classify_frame(samples: &[i16], cfg: &VadConfig, currently_speaking: bool) -> bool {
    let rms = frame_energy_rms(samples);
    let zcr = zero_crossing_rate(samples);
    let threshold = if currently_speaking {
        cfg.rms_close
    } else {
        cfg.rms_open
    };
    if rms < threshold {
        return false;
    }
    if zcr > cfg.zcr_noise_max && rms < cfg.rms_open {
        return false;
    }
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadDecision {
    Send,
    Drop,
}

#[derive(Debug, Clone)]
pub struct Vad {
    cfg: VadConfig,
    speaking: bool,
    attack_count: u16,
    hangover_count: u16,
    /// 自适应环境噪声底（慢速 EMA，仅在非语音帧更新）。
    noise_floor: u32,
}

impl Vad {
    pub fn new(cfg: VadConfig) -> Self {
        Self {
            cfg,
            speaking: false,
            attack_count: 0,
            hangover_count: 0,
            noise_floor: 0,
        }
    }

    pub fn is_speaking(&self) -> bool {
        self.speaking
    }

    /// 当前估计的环境噪声底（供诊断）。
    pub fn noise_floor(&self) -> u32 {
        self.noise_floor
    }

    /// 当前生效的开启门限（取绝对下限与“噪声底×margin”的较大者）。
    pub fn dynamic_open(&self) -> u32 {
        self.cfg
            .rms_open
            .max(self.noise_floor.saturating_mul(self.cfg.noise_margin_pct) / 100)
    }

    pub fn observe(&mut self, samples: &[i16]) -> VadDecision {
        let rms = frame_energy_rms(samples);
        let zcr = zero_crossing_rate(samples);
        let dynamic_open = self.dynamic_open();
        let threshold = if self.speaking {
            self.cfg.rms_close
        } else {
            dynamic_open
        };
        // 高过零率且能量不够强 → 视为噪声/底噪，拒绝；很响的真实语音仍放行。
        let noise_like = zcr > self.cfg.zcr_noise_max && rms < dynamic_open.saturating_mul(2);
        let voiced = rms >= threshold && !noise_like;

        // 跟踪环境底噪：未进入确认说话态时，遇更低能量立刻下探（抓安静基线），
        // 遇更高能量缓慢上抬（适应持续噪声）。说话期间冻结，避免语音污染底噪。
        if !self.speaking {
            self.noise_floor = if rms < self.noise_floor {
                rms // 快速下探，抓住安静基线（避免被一帧响声永久抬高）
            } else {
                self.noise_floor + (rms - self.noise_floor) / 64 // 缓慢上抬，适应持续噪声
            };
        }

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
        } else if voiced {
            self.hangover_count = self.cfg.hangover_frames;
            VadDecision::Send
        } else if self.hangover_count > 0 {
            self.hangover_count -= 1;
            VadDecision::Send
        } else {
            self.speaking = false;
            self.attack_count = 0;
            VadDecision::Drop
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
    pub fn record(&mut self, decision: VadDecision) {
        self.frames_total += 1;
        match decision {
            VadDecision::Send => self.frames_sent += 1,
            VadDecision::Drop => self.frames_dropped += 1,
        }
    }

    pub fn saved_ratio(&self) -> f64 {
        if self.frames_total == 0 {
            0.0
        } else {
            self.frames_dropped as f64 / self.frames_total as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(amp: i16, n: usize) -> Vec<i16> {
        (0..n)
            .map(|i| if i % 2 == 0 { amp } else { -amp })
            .collect()
    }

    #[test]
    fn silence_frame_is_dropped() {
        let mut vad = Vad::new(VadConfig::default_uplink());
        assert_eq!(vad.observe(&[0i16; 480]), VadDecision::Drop);
        let mut stats = VadStats::default();
        let mut v2 = Vad::new(VadConfig::default_uplink());
        for _ in 0..100 {
            stats.record(v2.observe(&[0i16; 480]));
        }
        assert_eq!(stats.frames_sent, 0);
        assert!((stats.saved_ratio() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn loud_speech_frame_is_sent_after_attack() {
        let cfg = VadConfig::default_uplink();
        let mut vad = Vad::new(cfg);
        let loud = tone(8000, 480);
        for _ in 0..cfg.attack_frames {
            let _ = vad.observe(&loud);
        }
        assert_eq!(vad.observe(&loud), VadDecision::Send);
        assert!(vad.is_speaking());
    }

    #[test]
    fn hysteresis_prevents_flapping_on_borderline_rms() {
        let cfg = VadConfig::default_uplink();
        let border = tone(450, 480);
        let mut from_silence = Vad::new(cfg);
        assert_eq!(from_silence.observe(&border), VadDecision::Drop);
        let mut speaking = Vad::new(cfg);
        let loud = tone(8000, 480);
        for _ in 0..(cfg.attack_frames + 1) {
            let _ = speaking.observe(&loud);
        }
        assert_eq!(speaking.observe(&border), VadDecision::Send);
    }

    #[test]
    fn hangover_keeps_sending_tail_after_speech_stops() {
        let cfg = VadConfig::default_uplink();
        let mut vad = Vad::new(cfg);
        let loud = tone(8000, 480);
        for _ in 0..(cfg.attack_frames + 1) {
            let _ = vad.observe(&loud);
        }
        let silence = [0i16; 480];
        for _ in 0..cfg.hangover_frames {
            assert_eq!(vad.observe(&silence), VadDecision::Send);
        }
        assert_eq!(vad.observe(&silence), VadDecision::Drop);
        assert!(!vad.is_speaking());
    }

    #[test]
    fn frame_energy_rms_is_pure_and_correct() {
        assert_eq!(frame_energy_rms(&[0i16; 256]), 0);
        assert!((frame_energy_rms(&[3000; 256]) as i32 - 3000).abs() <= 1);
        assert_eq!(frame_energy_rms(&[]), 0);
    }

    /// 低过零率、可控能量的块（半正半负），用于隔离能量/噪声底逻辑（不被 ZCR 门拦）。
    fn block(amp: i16, n: usize) -> Vec<i16> {
        (0..n).map(|i| if i < n / 2 { amp } else { -amp }).collect()
    }

    #[test]
    fn sustained_ambient_noise_below_threshold_never_opens() {
        let cfg = VadConfig::default_uplink();
        let mut vad = Vad::new(cfg);
        // 持续环境噪声，能量低于 rms_open（1000）。
        let noise = block(800, 480);
        let mut sends = 0;
        for _ in 0..200 {
            if vad.observe(&noise) == VadDecision::Send {
                sends += 1;
            }
        }
        assert_eq!(sends, 0, "环境噪声被误判为语音");
        assert!(!vad.is_speaking());
        // 噪声底应跟踪到接近噪声水平。
        assert!(vad.noise_floor() >= 600, "floor={}", vad.noise_floor());
    }

    #[test]
    fn loud_speech_opens_even_with_elevated_noise_floor() {
        let cfg = VadConfig::default_uplink();
        let mut vad = Vad::new(cfg);
        for _ in 0..100 {
            let _ = vad.observe(&block(800, 480)); // 抬高噪声底
        }
        let loud = block(9000, 480); // 远高于动态门限
        let mut opened = false;
        for _ in 0..(cfg.attack_frames + 1) {
            if vad.observe(&loud) == VadDecision::Send {
                opened = true;
            }
        }
        assert!(opened, "嘈杂环境下真实大声语音应能开启");
    }

    #[test]
    fn high_zcr_low_energy_classified_as_noise_not_speech() {
        let cfg = VadConfig::default_uplink();
        let noise = tone(200, 480);
        assert!(zero_crossing_rate(&noise) > cfg.zcr_noise_max);
        assert!(!classify_frame(&noise, &cfg, false));
        let mut vad = Vad::new(cfg);
        assert_eq!(vad.observe(&noise), VadDecision::Drop);
    }
}
