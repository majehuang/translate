//! 音频帧与采样率类型。

/// 单声道 16-bit PCM 帧，附带采样率，便于在管线中流动时自描述。
#[derive(Debug, Clone, PartialEq)]
pub struct PcmFrame {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}

impl PcmFrame {
    pub fn new(samples: Vec<i16>, sample_rate: u32) -> Self {
        Self {
            samples,
            sample_rate,
        }
    }

    /// 帧时长（毫秒）。
    pub fn duration_ms(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate as f64 * 1000.0
    }

    /// 转成小端字节序（Gemini 要求 16-bit little-endian）。
    pub fn to_le_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.samples.len() * 2);
        for s in &self.samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    /// 从小端字节序还原。
    pub fn from_le_bytes(bytes: &[u8], sample_rate: u32) -> Self {
        let samples = bytes
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        Self {
            samples,
            sample_rate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_of_16k_frame() {
        let frame = PcmFrame::new(vec![0i16; 1600], 16_000);
        assert!((frame.duration_ms() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn le_bytes_roundtrip() {
        let frame = PcmFrame::new(vec![-1, 0, 1, 256], 16_000);
        let bytes = frame.to_le_bytes();
        assert_eq!(bytes.len(), 8);
        let back = PcmFrame::from_le_bytes(&bytes, 16_000);
        assert_eq!(back, frame);
    }
}
