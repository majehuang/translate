//! 单条链路的独立运行单元：独立 Session、独立重连、独立背压、独立过期帧丢弃。
use crate::control::SessionState;
use crate::route::{LinkKind, LinkRole, RouteError};
use audio_core::{AudioBackend, PcmFrame, StreamCfg};
use audio_dsp::{Resampler, Vad, VadConfig, VadDecision};
use gemini_live::session::{connect_with_retry, SessionConfig};
use std::sync::Arc;
use tokio::sync::watch;
use tokio::task::AbortHandle;

const MODEL: &str = "models/gemini-3.5-live-translate-preview";
const GEMINI_IN_RATE: u32 = 16_000;
const GEMINI_OUT_RATE: u32 = 24_000;
const DEVICE_RATE: u32 = 48_000;
const DEVICE_FRAME: usize = 480;

pub struct LinkHandle {
    pub kind: LinkKind,
    pub state: watch::Receiver<SessionState>,
    pub(crate) abort: AbortHandle,
}

impl LinkHandle {
    pub fn current_state(&self) -> SessionState {
        self.state.borrow().clone()
    }

    pub fn abort(&self) {
        self.abort.abort();
    }
}

/// 启动一条链路：open_input/open_output → connect_with_retry → 单 task 内泵音频。
pub async fn spawn_link(
    role: &LinkRole,
    backend: Arc<dyn AudioBackend>,
    make_url: Arc<dyn Fn() -> String + Send + Sync>,
) -> Result<LinkHandle, RouteError> {
    let role = role.clone();
    let kind = role.kind;
    let (state_tx, state_rx) = watch::channel(SessionState::Starting);
    let handle = tokio::spawn(async move {
        run_link(role, backend, make_url, state_tx).await;
    });
    let abort = handle.abort_handle();
    Ok(LinkHandle {
        kind,
        state: state_rx,
        abort,
    })
}

async fn run_link(
    role: LinkRole,
    backend: Arc<dyn AudioBackend>,
    make_url: Arc<dyn Fn() -> String + Send + Sync>,
    state_tx: watch::Sender<SessionState>,
) {
    let cfg = StreamCfg {
        sample_rate: DEVICE_RATE,
        channels: 1,
        frame_size: DEVICE_FRAME,
    };

    let (input_stream, mut input) = match backend.open_input(&role.in_dev, cfg) {
        Ok(pair) => pair,
        Err(err) => {
            let _ = state_tx.send(SessionState::Error(err.to_string()));
            return;
        }
    };
    let (output_stream, mut output) = match backend.open_output(&role.out_dev, cfg) {
        Ok(pair) => pair,
        Err(err) => {
            let _ = state_tx.send(SessionState::Error(err.to_string()));
            return;
        }
    };

    let input_rate = input_stream.actual_sample_rate();
    let output_rate = output_stream.actual_sample_rate();
    let target_lang = role.target_lang.clone();
    let url_factory = make_url.clone();
    let session = connect_with_retry(
        || SessionConfig {
            url: url_factory(),
            model: MODEL.into(),
            out_rate: GEMINI_OUT_RATE,
            target_lang: target_lang.clone(),
            echo_target_language: false,
        },
        5,
    )
    .await;
    let (audio_tx, mut audio_rx) = match session {
        Ok(pair) => pair,
        Err(err) => {
            let _ = state_tx.send(SessionState::Error(err.to_string()));
            return;
        }
    };
    let _ = state_tx.send(SessionState::Running);

    let input_chunk = (input_rate / 100).max(1) as usize;
    let mut input_buf = vec![0i16; input_chunk];
    let mut vad = Vad::new(VadConfig::default_uplink());
    let mut up_resampler = Resampler::new(input_rate, GEMINI_IN_RATE, input_chunk);
    let mut down_resampler = Resampler::new(GEMINI_OUT_RATE, output_rate, DEVICE_FRAME);
    let _keep_alive = (input_stream, output_stream);

    loop {
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(5)) => {
                let got = input.pop_slice(&mut input_buf);
                if got == input_chunk && vad.observe(&input_buf) == VadDecision::Send {
                    let frame = PcmFrame::new(input_buf.clone(), input_rate);
                    let frame16 = up_resampler.process(&frame);
                    if audio_tx.send(frame16).await.is_err() {
                        let _ = state_tx.send(SessionState::Reconnecting { attempt: 1 });
                        break;
                    }
                }
            }
            maybe_frame = audio_rx.recv() => {
                let Some(frame24) = maybe_frame else {
                    let _ = state_tx.send(SessionState::Reconnecting { attempt: 1 });
                    break;
                };
                for chunk in frame24.samples.chunks(DEVICE_FRAME) {
                    let mut block = chunk.to_vec();
                    block.resize(DEVICE_FRAME, 0);
                    let frame = PcmFrame::new(block, GEMINI_OUT_RATE);
                    let out = down_resampler.process(&frame);
                    let _ = output.push_slice(&out.samples);
                }
            }
        }
    }
}
