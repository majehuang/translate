//! 采样率转换。设备常见 48k；Gemini 入 16k、出 24k。
use audio_core::PcmFrame;
use rubato::{FftFixedIn, Resampler as _};

pub struct Resampler {
    inner: FftFixedIn<f32>,
    from_rate: u32,
    to_rate: u32,
    chunk: usize,
}

impl Resampler {
    /// 创建定长输入重采样器。`chunk` 为每次处理的输入样本数。
    pub fn new(from_rate: u32, to_rate: u32, chunk: usize) -> Self {
        let inner = FftFixedIn::<f32>::new(from_rate as usize, to_rate as usize, chunk, 1, 1)
            .expect("创建重采样器");
        Self {
            inner,
            from_rate,
            to_rate,
            chunk,
        }
    }

    pub fn chunk_size(&self) -> usize {
        self.chunk
    }

    /// 处理一个恰好 chunk 长度的输入帧，返回目标采样率帧。
    pub fn process(&mut self, input: &PcmFrame) -> PcmFrame {
        assert_eq!(input.sample_rate, self.from_rate, "输入采样率不匹配");
        assert_eq!(input.samples.len(), self.chunk, "输入长度必须等于 chunk");
        let floats: Vec<f32> = input
            .samples
            .iter()
            .map(|sample| *sample as f32 / 32768.0)
            .collect();
        let out = self.inner.process(&[floats], None).expect("重采样");
        let samples: Vec<i16> = out[0]
            .iter()
            .map(|sample| (sample.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect();
        PcmFrame::new(samples, self.to_rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_core::PcmFrame;

    #[test]
    fn downsample_48k_to_16k_ratio() {
        let mut r = Resampler::new(48_000, 16_000, 480);
        let input = PcmFrame::new(vec![0i16; 480], 48_000);
        let out = r.process(&input);
        assert_eq!(out.sample_rate, 16_000);
        assert!(
            (out.samples.len() as i32 - 160).abs() <= 8,
            "got {}",
            out.samples.len()
        );
    }

    #[test]
    fn upsample_24k_to_48k_ratio() {
        let mut r = Resampler::new(24_000, 48_000, 480);
        let input = PcmFrame::new(vec![0i16; 480], 24_000);
        let out = r.process(&input);
        assert_eq!(out.sample_rate, 48_000);
        assert!(
            (out.samples.len() as i32 - 960).abs() <= 16,
            "got {}",
            out.samples.len()
        );
    }
}
