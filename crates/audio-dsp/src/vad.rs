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
    pub rms_open: u32,
    pub rms_close: u32,
    pub zcr_noise_max: u16,
    pub hangover_frames: u16,
    pub attack_frames: u16,
}

impl VadConfig {
    pub fn default_uplink() -> Self {
        Self {
            rms_open: 600,
            rms_close: 300,
            zcr_noise_max: 168,
            hangover_frames: 30,
            attack_frames: 2,
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
}

impl Vad {
    pub fn new(cfg: VadConfig) -> Self {
        Self {
            cfg,
            speaking: false,
            attack_count: 0,
            hangover_count: 0,
        }
    }

    pub fn is_speaking(&self) -> bool {
        self.speaking
    }

    pub fn observe(&mut self, samples: &[i16]) -> VadDecision {
        let voiced = classify_frame(samples, &self.cfg, self.speaking);
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
