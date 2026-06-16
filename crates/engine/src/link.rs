//! 单条链路的独立运行单元：独立 Session、独立重连、独立背压、独立过期帧丢弃。
use crate::control::SessionState;
use crate::route::{LinkKind, LinkRole, RouteError};
use audio_core::{AudioBackend, PcmFrame, StreamCfg};
use audio_dsp::{Resampler, Vad, VadConfig, VadDecision};
use gemini_live::session::{connect_with_retry, drop_stale_frames, SessionConfig, SessionError};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};
use tokio::task::AbortHandle;

const MODEL: &str = "models/gemini-3.5-live-translate-preview";
const GEMINI_IN_RATE: u32 = 16_000;
const GEMINI_OUT_RATE: u32 = 24_000;
const DEVICE_RATE: u32 = 48_000;
const DEVICE_FRAME: usize = 480;
const SESSION_CONNECT_ATTEMPTS: u32 = 5;
const MAX_RECONNECT_ATTEMPTS: u32 = 5;
const UPSTREAM_PENDING_KEEP: usize = 1;

type SessionIo = (mpsc::Sender<PcmFrame>, mpsc::Receiver<PcmFrame>);
type ConnectFuture = Pin<Box<dyn Future<Output = Result<SessionIo, SessionError>> + Send>>;
type SessionConnector = Arc<dyn Fn(SessionConfig) -> ConnectFuture + Send + Sync>;

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
    let connector: SessionConnector = Arc::new(|cfg| {
        Box::pin(connect_with_retry(
            move || SessionConfig {
                url: cfg.url.clone(),
                model: cfg.model.clone(),
                out_rate: cfg.out_rate,
                target_lang: cfg.target_lang.clone(),
                echo_target_language: cfg.echo_target_language,
            },
            SESSION_CONNECT_ATTEMPTS,
        ))
    });
    run_link_with_connector(role, backend, make_url, state_tx, connector).await;
}

async fn run_link_with_connector(
    role: LinkRole,
    backend: Arc<dyn AudioBackend>,
    make_url: Arc<dyn Fn() -> String + Send + Sync>,
    state_tx: watch::Sender<SessionState>,
    connector: SessionConnector,
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
    let input_chunk = (input_rate / 100).max(1) as usize;
    let mut input_buf = vec![0i16; input_chunk];
    let mut vad = Vad::new(VadConfig::default_uplink());
    let mut up_resampler = Resampler::new(input_rate, GEMINI_IN_RATE, input_chunk);
    let mut down_resampler = Resampler::new(GEMINI_OUT_RATE, output_rate, DEVICE_FRAME);
    let _keep_alive = (input_stream, output_stream);
    let mut reconnect = ReconnectBudget::new(MAX_RECONNECT_ATTEMPTS);

    loop {
        let cfg = SessionConfig {
            url: make_url(),
            model: MODEL.into(),
            out_rate: GEMINI_OUT_RATE,
            target_lang: target_lang.clone(),
            echo_target_language: false,
        };
        let (audio_tx, audio_rx) = match connector(cfg).await {
            Ok(pair) => pair,
            Err(err) => match reconnect.connect_failed(err.to_string()) {
                ReconnectDecision::Retry(state) => {
                    let _ = state_tx.send(state);
                    continue;
                }
                ReconnectDecision::GiveUp(state) => {
                    let _ = state_tx.send(state);
                    return;
                }
            },
        };
        reconnect.connected();
        let _ = state_tx.send(SessionState::Running);

        pump_session(
            audio_tx,
            audio_rx,
            &mut input,
            &mut output,
            &mut input_buf,
            &mut vad,
            &mut up_resampler,
            &mut down_resampler,
            input_rate,
        )
        .await;

        let _ = state_tx.send(reconnect.disconnected());
    }
}

struct ReconnectBudget {
    max_attempts: u32,
    next_attempt: u32,
}

enum ReconnectDecision {
    Retry(SessionState),
    GiveUp(SessionState),
}

impl ReconnectBudget {
    fn new(max_attempts: u32) -> Self {
        Self {
            max_attempts,
            next_attempt: 0,
        }
    }

    fn connected(&mut self) {
        self.next_attempt = 0;
    }

    fn disconnected(&mut self) -> SessionState {
        self.next_attempt = 1;
        SessionState::Reconnecting { attempt: 1 }
    }

    fn connect_failed(&mut self, error: String) -> ReconnectDecision {
        if self.next_attempt == 0 {
            return ReconnectDecision::GiveUp(SessionState::Error(error));
        }
        if self.next_attempt >= self.max_attempts {
            return ReconnectDecision::GiveUp(SessionState::Error(format!(
                "重连失败，已尝试 {} 次: {error}",
                self.next_attempt
            )));
        }
        self.next_attempt += 1;
        ReconnectDecision::Retry(SessionState::Reconnecting {
            attempt: self.next_attempt,
        })
    }
}

#[allow(clippy::too_many_arguments)]
async fn pump_session(
    audio_tx: mpsc::Sender<PcmFrame>,
    mut audio_rx: mpsc::Receiver<PcmFrame>,
    input: &mut audio_core::AudioConsumer,
    output: &mut audio_core::AudioProducer,
    input_buf: &mut [i16],
    vad: &mut Vad,
    up_resampler: &mut Resampler,
    down_resampler: &mut Resampler,
    input_rate: u32,
) {
    let input_chunk = input_buf.len();
    let mut pending_upstream = Vec::new();

    loop {
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(5)) => {
                if try_flush_upstream(&audio_tx, &mut pending_upstream).is_err() {
                    return;
                }
                let got = input.pop_slice(input_buf);
                if got == input_chunk && vad.observe(input_buf) == VadDecision::Send {
                    let frame = PcmFrame::new(input_buf.to_vec(), input_rate);
                    let frame16 = up_resampler.process(&frame);
                    if enqueue_latest_upstream(&audio_tx, &mut pending_upstream, frame16).is_err() {
                        return;
                    }
                }
            }
            maybe_frame = audio_rx.recv() => {
                let Some(frame24) = maybe_frame else {
                    return;
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

fn enqueue_latest_upstream(
    audio_tx: &mpsc::Sender<PcmFrame>,
    pending: &mut Vec<PcmFrame>,
    frame: PcmFrame,
) -> Result<(), ()> {
    // 上行不在 select 分支里 await：session 队列满时本地只保留最新帧，避免延迟堆积。
    pending.push(frame);
    drop_stale_frames(pending, UPSTREAM_PENDING_KEEP);
    try_flush_upstream(audio_tx, pending)
}

fn try_flush_upstream(
    audio_tx: &mpsc::Sender<PcmFrame>,
    pending: &mut Vec<PcmFrame>,
) -> Result<(), ()> {
    while !pending.is_empty() {
        let frame = pending.remove(0);
        match audio_tx.try_send(frame) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(frame)) => {
                pending.insert(0, frame);
                drop_stale_frames(pending, UPSTREAM_PENDING_KEEP);
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

    #[test]
    fn reconnect_budget_retries_then_returns_to_running_budget() {
        let mut budget = ReconnectBudget::new(3);

        assert_eq!(
            budget.disconnected(),
            SessionState::Reconnecting { attempt: 1 }
        );
        assert!(matches!(
            budget.connect_failed("net".into()),
            ReconnectDecision::Retry(SessionState::Reconnecting { attempt: 2 })
        ));
        assert!(matches!(
            budget.connect_failed("net".into()),
            ReconnectDecision::Retry(SessionState::Reconnecting { attempt: 3 })
        ));
        assert!(matches!(
            budget.connect_failed("net".into()),
            ReconnectDecision::GiveUp(SessionState::Error(_))
        ));

        budget.connected();
        assert_eq!(
            budget.disconnected(),
            SessionState::Reconnecting { attempt: 1 }
        );
    }

    #[test]
    fn upstream_backpressure_keeps_latest_pending_frame() {
        let (audio_tx, mut audio_rx) = mpsc::channel(1);
        let mut pending = Vec::new();

        enqueue_latest_upstream(&audio_tx, &mut pending, frame(1)).unwrap();
        enqueue_latest_upstream(&audio_tx, &mut pending, frame(2)).unwrap();
        enqueue_latest_upstream(&audio_tx, &mut pending, frame(3)).unwrap();

        assert_eq!(audio_rx.try_recv().unwrap().samples[0], 1);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].samples[0], 3);
    }

    fn frame(sample: i16) -> PcmFrame {
        PcmFrame::new(vec![sample; 160], GEMINI_IN_RATE)
    }
}
