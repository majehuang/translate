//! 用本地 WS 服务器模拟 Gemini：收到 setup 后回一帧音频，验证收发闭环。
use audio_core::PcmFrame;
use futures_util::{SinkExt, StreamExt};
use gemini_live::session::{connect, SessionConfig};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn session_sends_setup_and_receives_audio() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let first = ws.next().await.unwrap().unwrap();
        assert!(first.into_text().unwrap().contains("generationConfig"));
        let resp = r#"{"serverContent":{"modelTurn":{"parts":[
            {"inlineData":{"mimeType":"audio/pcm;rate=24000","data":"AQACAA=="}}]}}}"#;
        ws.send(Message::Text(resp.to_string().into())).await.unwrap();
        let _ = ws.next().await;
    });

    let (tx, mut rx) = connect(SessionConfig {
        url: format!("ws://{addr}"),
        model: "models/test".into(),
        out_rate: 24_000,
    })
    .await
    .unwrap();

    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("超时")
        .expect("通道关闭");
    assert_eq!(frame.sample_rate, 24_000);
    assert_eq!(frame.samples, vec![1, 2]);

    tx.send(PcmFrame::new(vec![0; 160], 16_000)).await.unwrap();
    server.await.unwrap();
}
