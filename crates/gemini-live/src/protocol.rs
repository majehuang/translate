//! Gemini Live API (BidiGenerateContent) WebSocket 消息类型。
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Setup {
    pub setup: SetupBody,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupBody {
    pub model: String,
    pub generation_config: GenerationConfig,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationConfig {
    pub response_modalities: Vec<String>,
    pub translation_config: TranslationConfig,
}

/// Live Translate 配置。auto 模式：只锁目标语言，源语言由模型自动识别。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationConfig {
    /// 目标语言 BCP-47 码，如 "en"/"zh"。
    pub target_language_code: String,
    /// 输入已是目标语言时是否原样回放（true）还是静音（false）。
    pub echo_target_language: bool,
}

/// 实时音频输入帧。audio 为 base64 的 16k/16-bit/mono/LE PCM。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeInput {
    pub realtime_input: RealtimeInputBody,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeInputBody {
    pub audio: AudioBlob,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioBlob {
    pub mime_type: String,
    pub data: String,
}

/// 服务端响应（部分字段，按需扩展）。
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerMessage {
    #[serde(default)]
    pub server_content: Option<ServerContent>,
    #[serde(default)]
    pub setup_complete: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerContent {
    #[serde(default)]
    pub model_turn: Option<ModelTurn>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelTurn {
    #[serde(default)]
    pub parts: Vec<Part>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    #[serde(default)]
    pub inline_data: Option<InlineData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineData {
    pub mime_type: String,
    pub data: String,
}

impl Setup {
    /// 构造 Live Translate 的 setup。auto 模式下 source 不写入（由模型识别），
    /// 只锁目标语言 `target_lang`（BCP-47）。`echo` 控制同语言时回放/静音。
    pub fn new_translate(model: &str, target_lang: &str, echo: bool) -> Self {
        Setup {
            setup: SetupBody {
                model: model.to_string(),
                generation_config: GenerationConfig {
                    response_modalities: vec!["AUDIO".to_string()],
                    translation_config: TranslationConfig {
                        target_language_code: target_lang.to_string(),
                        echo_target_language: echo,
                    },
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_serializes_with_camel_case() {
        let s = Setup::new_translate("models/gemini-3.5-live-translate-preview", "en", true);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(
            json["setup"]["model"],
            "models/gemini-3.5-live-translate-preview"
        );
        assert_eq!(
            json["setup"]["generationConfig"]["responseModalities"][0],
            "AUDIO"
        );
        // 真实 API 校验：translationConfig 必须嵌在 generationConfig 内（见 examples/smoke 实测）。
        let tc = &json["setup"]["generationConfig"]["translationConfig"];
        assert_eq!(tc["targetLanguageCode"], "en");
        assert_eq!(tc["echoTargetLanguage"], true);
    }

    #[test]
    fn setup_omits_source_language_in_auto_mode() {
        // auto 模式：不应出现任何 source 字段，源语言交给模型识别。
        let s = Setup::new_translate("models/gemini-3.5-live-translate-preview", "zh", false);
        let json = serde_json::to_value(&s).unwrap();
        assert!(json["setup"].get("sourceLanguageCode").is_none());
        assert!(json["setup"]["generationConfig"].get("source").is_none());
    }

    #[test]
    fn realtime_input_serializes() {
        let ri = RealtimeInput {
            realtime_input: RealtimeInputBody {
                audio: AudioBlob {
                    mime_type: "audio/pcm;rate=16000".into(),
                    data: "AAAA".into(),
                },
            },
        };
        let json = serde_json::to_value(&ri).unwrap();
        assert_eq!(
            json["realtimeInput"]["audio"]["mimeType"],
            "audio/pcm;rate=16000"
        );
        assert_eq!(json["realtimeInput"]["audio"]["data"], "AAAA");
    }

    #[test]
    fn server_message_parses_audio_response() {
        let raw = r#"{
            "serverContent": {
                "modelTurn": {
                    "parts": [
                        {"inlineData": {"mimeType": "audio/pcm;rate=24000", "data": "QUJD"}}
                    ]
                }
            }
        }"#;
        let msg: ServerMessage = serde_json::from_str(raw).unwrap();
        let data = msg.server_content.unwrap().model_turn.unwrap().parts[0]
            .inline_data
            .as_ref()
            .unwrap()
            .data
            .clone();
        assert_eq!(data, "QUJD");
    }

    #[test]
    fn server_message_tolerates_unknown_fields() {
        let raw = r#"{"serverContent":{"turnComplete":true},"usageMetadata":{"x":1}}"#;
        let _msg: ServerMessage = serde_json::from_str(raw).unwrap();
    }
}
