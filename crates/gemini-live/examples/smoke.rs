//! 一次性协议核对 smoke 测试（Task 13 Step 1，无需麦克风）。
//! 连接真实 Gemini Live → 发 setup + 1 秒静音(16k) → 打印前若干条原始服务端帧。
//! 用法：GEMINI_API_KEY=xxx cargo run -p gemini-live --example smoke
//! 注意：绝不打印含 key 的 URL。
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

const MODEL: &str = "models/gemini-3.5-live-translate-preview";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("GEMINI_API_KEY").map_err(|_| "缺少 GEMINI_API_KEY")?;
    gemini_live::session::ensure_crypto_provider();

    // API Key 鉴权用 ?key=（access_token= 是 OAuth bearer，AI Studio key 不适用）。
    let url = format!(
        "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key={key}"
    );
    eprintln!("连接中（URL 含 key，已隐藏）…");
    let (ws, resp) = tokio_tungstenite::connect_async(&url).await?;
    eprintln!("握手成功，HTTP {}", resp.status());
    let (mut write, mut read) = ws.split();

    // setup：显式带 targetLanguageCode（核对真实是否接受此字段）。
    let setup = serde_json::json!({
        "setup": {
            "model": MODEL,
            "generationConfig": {
                "responseModalities": ["AUDIO"],
                "translationConfig": {
                    "targetLanguageCode": "en",
                    "echoTargetLanguage": true
                }
            }
        }
    });
    write
        .send(Message::Text(serde_json::to_string(&setup)?))
        .await?;
    eprintln!("已发送 setup（target=en）");

    // 1 秒 16k 静音，分成 100ms 帧发送。
    let silence = vec![0u8; 16000 * 2 / 10]; // 100ms @16k 16-bit mono
    let b64 = STANDARD.encode(&silence);
    for _ in 0..10 {
        let frame = serde_json::json!({
            "realtimeInput": { "audio": { "mimeType": "audio/pcm;rate=16000", "data": b64 } }
        });
        write
            .send(Message::Text(serde_json::to_string(&frame)?))
            .await?;
    }
    eprintln!("已发送 1s 静音，开始读取服务端帧（最多 15s / 20 帧）…\n");

    let mut n = 0;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    while n < 20 {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, read.next()).await {
            Ok(Some(Ok(msg))) => {
                let text = match msg {
                    Message::Text(t) => t.to_string(),
                    Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
                    Message::Close(c) => {
                        println!("[CLOSE] {c:?}");
                        break;
                    }
                    _ => continue,
                };
                n += 1;
                // 截断超长音频 base64，只看结构。
                let shown: String = text.chars().take(600).collect();
                println!("--- 帧 #{n} ({} 字节) ---\n{shown}\n", text.len());
            }
            Ok(Some(Err(e))) => {
                println!("[读取错误] {e}");
                break;
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    eprintln!("\n完成，共收到 {n} 帧。");
    Ok(())
}
