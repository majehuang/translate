//! 无锁环形缓冲封装：生产者在音频回调线程，消费者在处理线程。
//! 基于 ringbuf 0.4 的普通 `Producer::push_slice`：缓冲满时只写入可容纳
//! 的前缀，溢出的新样本被丢弃，不覆盖旧样本，并且不阻塞回调线程。

use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;

pub struct AudioProducer {
    inner: <HeapRb<i16> as Split>::Prod,
}

pub struct AudioConsumer {
    inner: <HeapRb<i16> as Split>::Cons,
}

/// 创建容量为 `capacity` 个 i16 样本的环形缓冲。
pub fn audio_channel(capacity: usize) -> (AudioProducer, AudioConsumer) {
    let (prod, cons) = HeapRb::<i16>::new(capacity).split();
    (AudioProducer { inner: prod }, AudioConsumer { inner: cons })
}

impl AudioProducer {
    /// 推入样本；缓冲不够时只写入可容纳部分，丢弃溢出的新样本，返回实际写入数。
    /// 关键：绝不阻塞，供音频回调线程安全调用。
    pub fn push_slice(&mut self, data: &[i16]) -> usize {
        self.inner.push_slice(data)
    }
}

impl AudioConsumer {
    /// 拉出最多 `out.len()` 个样本，返回实际读取数。
    pub fn pop_slice(&mut self, out: &mut [i16]) -> usize {
        self.inner.pop_slice(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_then_pop_roundtrip() {
        let (mut prod, mut cons) = audio_channel(8);
        assert_eq!(prod.push_slice(&[1, 2, 3]), 3);
        let mut out = [0i16; 4];
        assert_eq!(cons.pop_slice(&mut out), 3);
        assert_eq!(&out[..3], &[1, 2, 3]);
    }

    #[test]
    fn push_drops_overflow_without_blocking() {
        let (mut prod, mut cons) = audio_channel(4);
        // 容量 4，推 6 个 -> 只写入 4，溢出的新样本被丢弃，不阻塞。
        assert_eq!(prod.push_slice(&[1, 2, 3, 4, 5, 6]), 4);
        let mut out = [0i16; 4];
        assert_eq!(cons.pop_slice(&mut out), 4);
        assert_eq!(out, [1, 2, 3, 4]);
    }
}
