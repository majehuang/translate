//! M1 验证 CLI：物理麦 → 重采样 16k → Gemini Live Translate → 24k 播放到扬声器。
//! 用法：
//!   GEMINI_API_KEY=xxx cargo run -p cli -- --target en --in-device "<麦克风名>" --out-device "<扬声器名>"
//! 不带 --in-device 时列出设备并退出。
use audio_core::{AudioBackend, DeviceId, PcmFrame, StreamCfg};
use audio_cpal::CpalBackend;
use audio_dsp::Resampler;
use gemini_live::session::{connect_with_retry, SessionConfig};
use std::time::Instant;

const MODEL: &str = "models/gemini-3.5-live-translate";
const GEMINI_IN_RATE: u32 = 16_000;
const GEMINI_OUT_RATE: u32 = 24_000;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args: Vec<String> = std::env::args().collect();
    let backend = CpalBackend::new();

    let in_device = arg_value(&args, "--in-device");
    let out_device = arg_value(&args, "--out-device");
    let target = arg_value(&args, "--target").unwrap_or_else(|| "en".to_string());

    if in_device.is_none() {
        println!("== 输入设备 ==");
        for device in backend.list_inputs()? {
            println!(
                "  {}{}",
                device.name,
                if device.is_default { " [默认]" } else { "" }
            );
        }
        println!("== 输出设备 ==");
        for device in backend.list_outputs()? {
            println!(
                "  {}{}",
                device.name,
                if device.is_default { " [默认]" } else { "" }
            );
        }
        println!("\n请用 --in-device/--out-device 指定设备后重跑。");
        return Ok(());
    }

    let out_device = out_device.ok_or_else(|| anyhow::anyhow!("缺少 --out-device 输出设备参数"))?;
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| anyhow::anyhow!("缺少 GEMINI_API_KEY 环境变量"))?;
    let url = format!(
        "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?access_token={api_key}"
    );

    let cfg = StreamCfg {
        sample_rate: 48_000,
        channels: 1,
        frame_size: 480,
    };
    let (in_stream, mut mic_cons) =
        backend.open_input(&DeviceId(in_device.expect("checked above")), cfg)?;
    let (out_stream, mut spk_prod) = backend.open_output(&DeviceId(out_device), cfg)?;
    let in_rate = in_stream.actual_sample_rate();
    let out_rate = out_stream.actual_sample_rate();
    tracing::info!(in_rate, out_rate, %target, "音频设备已打开");

    let (audio_tx, mut audio_rx) = connect_with_retry(
        || SessionConfig {
            url: url.clone(),
            model: MODEL.into(),
            out_rate: GEMINI_OUT_RATE,
        },
        5,
    )
    .await
    .map_err(|err| anyhow::anyhow!("Gemini 连接失败: {err}"))?;
    tracing::info!("Gemini 已连接");

    let up_chunk = (in_rate / 100) as usize;
    let mut up_resampler = Resampler::new(in_rate, GEMINI_IN_RATE, up_chunk);
    let send_started = Instant::now();
    tokio::spawn(async move {
        let mut buf = vec![0i16; up_chunk];
        loop {
            let got = mic_cons.pop_slice(&mut buf);
            if got == up_chunk {
                let frame = PcmFrame::new(buf.clone(), in_rate);
                let frame16 = up_resampler.process(&frame);
                if audio_tx.send(frame16).await.is_err() {
                    break;
                }
            } else {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
    });

    let mut down_resampler = Resampler::new(GEMINI_OUT_RATE, out_rate, 480);
    let mut first_audio_logged = false;
    tokio::spawn(async move {
        while let Some(frame24) = audio_rx.recv().await {
            if !first_audio_logged {
                let latency_ms = send_started.elapsed().as_millis();
                tracing::info!(latency_ms, "首个翻译音频到达");
                first_audio_logged = true;
            }
            for chunk in frame24.samples.chunks(480) {
                let mut block = chunk.to_vec();
                block.resize(480, 0);
                let frame = PcmFrame::new(block, GEMINI_OUT_RATE);
                let out = down_resampler.process(&frame);
                let _ = spk_prod.push_slice(&out.samples);
            }
        }
    });

    println!("运行中，对着麦克风说话；Ctrl+C 停止。");
    tokio::signal::ctrl_c().await?;
    drop(in_stream);
    drop(out_stream);
    println!("已停止。");
    Ok(())
}

fn arg_value(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == key)
        .and_then(|index| args.get(index + 1).cloned())
}
