//! 逐帧能量摘要：数据面热路径调用，O(n) 累加、无堆分配、无锁、无 await。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameEnergy {
    pub rms_q15: i32,
    pub peak: i16,
    pub n: u16,
}

pub fn frame_energy(samples: &[i16]) -> FrameEnergy {
    if samples.is_empty() {
        return FrameEnergy {
            rms_q15: 0,
            peak: 0,
            n: 0,
        };
    }
    let mut acc: i64 = 0;
    let mut peak: i16 = 0;
    for &sample in samples {
        acc += (sample as i64) * (sample as i64);
        let abs = sample.saturating_abs();
        if abs > peak {
            peak = abs;
        }
    }
    let mean = acc / samples.len() as i64;
    let rms = (mean as f64).sqrt() as i32;
    FrameEnergy {
        rms_q15: rms,
        peak,
        n: samples.len() as u16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_energy_is_zero_alloc_and_correct() {
        assert_eq!(
            frame_energy(&[0i16; 480]),
            FrameEnergy {
                rms_q15: 0,
                peak: 0,
                n: 480
            }
        );
        let e = frame_energy(&[i16::MAX; 480]);
        assert_eq!(e.peak, i16::MAX);
        assert!(
            e.rms_q15 > 32000,
            "满刻度 RMS 接近 i16::MAX, got {}",
            e.rms_q15
        );
        assert_eq!(frame_energy(&[]).n, 0);
    }
}
