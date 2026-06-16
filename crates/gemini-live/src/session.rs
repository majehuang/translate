//! Gemini Live Session：管理一条 WebSocket，发送音频、接收翻译音频。
use crate::codec::{decode_audio, encode_input};
use crate::protocol::{ServerMessage, Setup};
use audio_core::PcmFrame;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

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
}

/// 启动一条 Session。返回上行音频 sender 与下行音频 receiver。
pub async fn connect(
    cfg: SessionConfig,
) -> Result<(mpsc::Sender<PcmFrame>, mpsc::Receiver<PcmFrame>), SessionError> {
    let (ws, _) = tokio_tungstenite::connect_async(&cfg.url)
        .await
        .map_err(|e| SessionError::Connect(e.to_string()))?;
    let (mut write, mut read) = ws.split();

    let setup = Setup::new_translate(&cfg.model);
    let setup_json = serde_json::to_string(&setup).expect("setup serialize");
    write
        .send(Message::Text(setup_json.into()))
        .await
        .map_err(|e| SessionError::Send(e.to_string()))?;

    let (audio_tx, mut audio_in) = mpsc::channel::<PcmFrame>(64);
    let (audio_out, audio_rx) = mpsc::channel::<PcmFrame>(64);

    tokio::spawn(async move {
        while let Some(frame) = audio_in.recv().await {
            let ri = encode_input(&frame);
            let json = match serde_json::to_string(&ri) {
                Ok(json) => json,
                Err(_) => continue,
            };
            if write.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    let out_rate = cfg.out_rate;
    tokio::spawn(async move {
        while let Some(Ok(msg)) = read.next().await {
            let text = match msg {
                Message::Text(text) => text.to_string(),
                Message::Binary(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Message::Close(_) => break,
                _ => continue,
            };
            if let Ok(server_msg) = serde_json::from_str::<ServerMessage>(&text) {
                for frame in decode_audio(&server_msg, out_rate) {
                    if audio_out.send(frame).await.is_err() {
                        return;
                    }
                }
            }
        }
    });

    Ok((audio_tx, audio_rx))
}
