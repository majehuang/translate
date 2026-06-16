//! PcmFrame ↔ Gemini 协议消息的编解码。
use crate::protocol::{AudioBlob, RealtimeInput, RealtimeInputBody, ServerMessage};
use audio_core::PcmFrame;
use base64::{engine::general_purpose::STANDARD, Engine};

/// 16k PcmFrame → realtimeInput 消息。
pub fn encode_input(frame: &PcmFrame) -> RealtimeInput {
    let b64 = STANDARD.encode(frame.to_le_bytes());
    RealtimeInput {
        realtime_input: RealtimeInputBody {
            audio: AudioBlob {
                mime_type: format!("audio/pcm;rate={}", frame.sample_rate),
                data: b64,
            },
        },
    }
}

/// 从一条 ServerMessage 抽取所有音频 part，解码为目标采样率 PcmFrame。
pub fn decode_audio(msg: &ServerMessage, out_rate: u32) -> Vec<PcmFrame> {
    let mut frames = Vec::new();
    if let Some(sc) = &msg.server_content {
        if let Some(mt) = &sc.model_turn {
            for part in &mt.parts {
                if let Some(inline) = &part.inline_data {
                    if inline.mime_type.starts_with("audio/pcm") {
                        if let Ok(bytes) = STANDARD.decode(&inline.data) {
                            frames.push(PcmFrame::from_le_bytes(&bytes, out_rate));
                        }
                    }
                }
            }
        }
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_core::PcmFrame;
    use base64::{engine::general_purpose::STANDARD, Engine};

    #[test]
    fn encode_then_manual_decode_roundtrip() {
        let frame = PcmFrame::new(vec![1, -1, 100, -100], 16_000);
        let ri = encode_input(&frame);
        assert_eq!(ri.realtime_input.audio.mime_type, "audio/pcm;rate=16000");
        let bytes = STANDARD.decode(&ri.realtime_input.audio.data).unwrap();
        assert_eq!(PcmFrame::from_le_bytes(&bytes, 16_000), frame);
    }

    #[test]
    fn decode_audio_extracts_frame() {
        let raw = r#"{"serverContent":{"modelTurn":{"parts":[
            {"inlineData":{"mimeType":"audio/pcm;rate=24000","data":"AQACAA=="}}
        ]}}}"#;
        let msg: crate::protocol::ServerMessage = serde_json::from_str(raw).unwrap();
        let frames = decode_audio(&msg, 24_000);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].sample_rate, 24_000);
        assert_eq!(frames[0].samples, vec![1, 2]);
    }

    #[test]
    fn decode_audio_empty_when_no_content() {
        let msg = crate::protocol::ServerMessage::default();
        assert!(decode_audio(&msg, 24_000).is_empty());
    }
}
