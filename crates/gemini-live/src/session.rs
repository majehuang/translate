//! Gemini Live Session：管理一条 WebSocket，发送音频、接收翻译音频。
use crate::codec::{decode_audio, encode_input};
use crate::protocol::{ServerMessage, Setup};
use audio_core::PcmFrame;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

const DOWNSTREAM_CHANNEL_CAPACITY: usize = 4;
const DOWNSTREAM_PENDING_KEEP: usize = 1;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("连接失败: {0}")]
    Connect(String),
    #[error("发送失败: {0}")]
    Send(String),
}

pub struct SessionConfig {
    pub url: String,
    pub model: String,
    pub out_rate: u32,
    /// 目标语言 BCP-47 码（听者语言）。
    pub target_lang: String,
    /// 输入已是目标语言时回放(true)/静音(false)。
    pub echo_target_language: bool,
}

/// 进程级安装 rustls 的 ring crypto provider。
///
/// rustls 0.23 起不再自动选择 crypto provider；真实 wss:// 握手前必须安装，
/// 否则 tokio-tungstenite 在建连时会 panic。用 `Once` 保证只装一次、可被多处
/// （`connect` 与示例程序）安全重复调用。
pub fn ensure_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // 已安装则忽略错误（例如宿主程序已自行安装其它 provider）。
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// 队列超过 `keep` 时，丢弃最旧帧只保留最新 `keep` 帧。
pub fn drop_stale_frames(queue: &mut Vec<PcmFrame>, keep: usize) {
    if queue.len() > keep {
        let drop_n = queue.len() - keep;
        queue.drain(0..drop_n);
    }
}

/// 带指数退避的连接：失败时按 0.5s, 1s, 2s, 4s... 重试。
pub async fn connect_with_retry(
    make_cfg: impl Fn() -> SessionConfig,
    max_attempts: u32,
) -> Result<(mpsc::Sender<PcmFrame>, mpsc::Receiver<PcmFrame>), SessionError> {
    let mut delay_ms = 500u64;
    let mut last_err = None;
    for attempt in 0..max_attempts {
        match connect(make_cfg()).await {
            Ok(pair) => return Ok(pair),
            Err(err) => {
                tracing::warn!(attempt, error = %err, "连接失败，准备重试");
                last_err = Some(err);
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                delay_ms = (delay_ms * 2).min(8_000);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| SessionError::Connect("超出最大重试次数".into())))
}

/// 启动一条 Session。返回上行音频 sender 与下行音频 receiver。
pub async fn connect(
    cfg: SessionConfig,
) -> Result<(mpsc::Sender<PcmFrame>, mpsc::Receiver<PcmFrame>), SessionError> {
    ensure_crypto_provider();
    let (ws, _) = tokio_tungstenite::connect_async(&cfg.url)
        .await
        .map_err(|e| SessionError::Connect(e.to_string()))?;
    let (mut write, mut read) = ws.split();

    let setup = Setup::new_translate(&cfg.model, &cfg.target_lang, cfg.echo_target_language);
    let setup_json = serde_json::to_string(&setup).expect("setup serialize");
    write
        .send(Message::Text(setup_json))
        .await
        .map_err(|e| SessionError::Send(e.to_string()))?;

    let (audio_tx, mut audio_in) = mpsc::channel::<PcmFrame>(64);
    let (audio_out, audio_rx) = mpsc::channel::<PcmFrame>(DOWNSTREAM_CHANNEL_CAPACITY);

    tokio::spawn(async move {
        while let Some(frame) = audio_in.recv().await {
            let ri = encode_input(&frame);
            let json = match serde_json::to_string(&ri) {
                Ok(json) => json,
                Err(_) => continue,
            };
            if write.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
    });

    let out_rate = cfg.out_rate;
    tokio::spawn(async move {
        let mut pending_out = Vec::new();
        let mut flush_tick = tokio::time::interval(std::time::Duration::from_millis(5));
        flush_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = flush_tick.tick() => {
                    if flush_lossy_output(&audio_out, &mut pending_out).is_err() {
                        return;
                    }
                }
                maybe_msg = read.next() => {
                    let Some(result) = maybe_msg else {
                        return;
                    };
                    let msg = match result {
                        Ok(msg) => msg,
                        Err(err) => {
                            tracing::warn!(error = %err, "Gemini Live 下行读取失败");
                            return;
                        }
                    };
                    let text = match msg {
                        Message::Text(text) => text.to_string(),
                        Message::Binary(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                        Message::Close(_) => return,
                        _ => continue,
                    };
                    if let Ok(server_msg) = serde_json::from_str::<ServerMessage>(&text) {
                        for frame in decode_audio(&server_msg, out_rate) {
                            if try_send_lossy_output(&audio_out, &mut pending_out, frame).is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        }
    });

    Ok((audio_tx, audio_rx))
}

fn try_send_lossy_output(
    audio_out: &mpsc::Sender<PcmFrame>,
    pending: &mut Vec<PcmFrame>,
    frame: PcmFrame,
) -> Result<(), ()> {
    pending.push(frame);
    drop_stale_frames(pending, DOWNSTREAM_PENDING_KEEP);
    flush_lossy_output(audio_out, pending)
}

fn flush_lossy_output(
    audio_out: &mpsc::Sender<PcmFrame>,
    pending: &mut Vec<PcmFrame>,
) -> Result<(), ()> {
    while !pending.is_empty() {
        let frame = pending.remove(0);
        match audio_out.try_send(frame) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(frame)) => {
                pending.insert(0, frame);
                drop_stale_frames(pending, DOWNSTREAM_PENDING_KEEP);
                return Ok(());
            }
            Err(mpsc::error::TrySendError::Closed(_)) => return Err(()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_core::PcmFrame;

    #[test]
    fn drop_stale_keeps_latest() {
        let mut q: Vec<PcmFrame> = (0..10)
            .map(|i| PcmFrame::new(vec![i as i16], 16_000))
            .collect();
        drop_stale_frames(&mut q, 3);
        assert_eq!(q.len(), 3);
        assert_eq!(q[0].samples[0], 7);
        assert_eq!(q[2].samples[0], 9);
    }

    #[test]
    fn drop_stale_noop_when_under_limit() {
        let mut q: Vec<PcmFrame> = (0..2)
            .map(|i| PcmFrame::new(vec![i as i16], 16_000))
            .collect();
        drop_stale_frames(&mut q, 5);
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn lossy_output_send_keeps_latest_pending_frame() {
        let (tx, mut rx) = mpsc::channel(1);
        let mut pending = Vec::new();

        try_send_lossy_output(&tx, &mut pending, frame(1)).unwrap();
        try_send_lossy_output(&tx, &mut pending, frame(2)).unwrap();
        try_send_lossy_output(&tx, &mut pending, frame(3)).unwrap();

        assert_eq!(rx.try_recv().unwrap().samples[0], 1);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].samples[0], 3);

        flush_lossy_output(&tx, &mut pending).unwrap();
        assert_eq!(rx.try_recv().unwrap().samples[0], 3);
    }

    fn frame(sample: i16) -> PcmFrame {
        PcmFrame::new(vec![sample; 160], 16_000)
    }
}
