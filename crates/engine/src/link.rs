//! 单条链路的独立运行单元：独立 Session、独立重连、独立背压、独立过期帧丢弃。
use crate::control::{ControlEvent, PauseReason, SessionState};
use crate::route::{LinkKind, LinkRole, RouteError};
use audio_core::{AudioBackend, PcmFrame, StreamCfg};
use audio_dsp::{Resampler, Vad, VadConfig, VadDecision};
use diagnostics::{
    detect_loop, frame_energy, step_guard, FrameEnergy, GuardAction, LoopEvidence, LoopGuardState,
    LoopThresholds,
};
use gemini_live::session::{connect_with_retry, drop_stale_frames, SessionConfig, SessionError};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
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
// 下行播放缓冲上限：必须 ≥ 一句译文的时长，否则会把整句截短（Gemini 按整句返回多秒音频）。
// 设为 1s，仅在跨句严重积压时丢旧，不截断当前句。
const DOWNSTREAM_PENDING_MS: usize = 1000;
const LOOP_WINDOW_FRAMES: usize = 64;
const DIAGNOSTIC_LOG_INTERVAL: Duration = Duration::from_secs(5);

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
    evt_tx: mpsc::Sender<ControlEvent>,
) -> Result<LinkHandle, RouteError> {
    let role = role.clone();
    let kind = role.kind;
    let (state_tx, state_rx) = watch::channel(SessionState::Starting);
    let handle = tokio::spawn(async move {
        run_link(role, backend, make_url, state_tx, evt_tx).await;
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
    evt_tx: mpsc::Sender<ControlEvent>,
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
    run_link_with_connector(role, backend, make_url, state_tx, evt_tx, connector).await;
}

async fn run_link_with_connector(
    role: LinkRole,
    backend: Arc<dyn AudioBackend>,
    make_url: Arc<dyn Fn() -> String + Send + Sync>,
    state_tx: watch::Sender<SessionState>,
    evt_tx: mpsc::Sender<ControlEvent>,
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
            output_rate,
            role.kind,
            &state_tx,
            &evt_tx,
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
    output_rate: u32,
    kind: LinkKind,
    state_tx: &watch::Sender<SessionState>,
    evt_tx: &mpsc::Sender<ControlEvent>,
) {
    let input_chunk = input_buf.len();
    let mut pending_upstream = Vec::new();
    let mut pending_downstream = Vec::new();
    let max_pending_downstream = samples_for_ms(output_rate, DOWNSTREAM_PENDING_MS);
    let mut downstream_dropped_samples = 0u64;
    let mut last_diag_log = Instant::now();
    let mut loop_guard = LoopGuardRuntime::new(conservative_loop_thresholds(), LOOP_WINDOW_FRAMES);
    // 实验开关：设 TRANSLATE_NO_VAD=1 则不丢静音、发连续音频，交给 Gemini 服务端做 turn 检测。
    // 用于排查"同一句译文重复多遍"是否由客户端 VAD 切碎语音、破坏服务端断句导致。
    let vad_disabled = std::env::var("TRANSLATE_NO_VAD").is_ok();
    if vad_disabled {
        tracing::info!(?kind, "上行 VAD 已禁用（发连续音频）");
    }
    // 回环检测默认关闭：当前 detect_loop 在长时间静音段可能误判（近零信号互相关偏高）
    // 而误暂停整条链路、吞掉全部音频。设 TRANSLATE_LOOPGUARD=1 才启用。无声学环路时无需开。
    let loopguard_enabled = std::env::var("TRANSLATE_LOOPGUARD").is_ok();
    if loopguard_enabled {
        tracing::info!(?kind, "回环检测已启用");
    }

    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(5)) => {
                flush_downstream(output, &mut pending_downstream);
                if try_flush_upstream(&audio_tx, &mut pending_upstream).is_err() {
                    return;
                }
                let got = input.pop_slice(input_buf);
                if got == input_chunk {
                    if loopguard_enabled {
                        if let Some(action) = loop_guard.observe_captured(input_buf) {
                            handle_loop_action(action, kind, &loop_guard, state_tx, evt_tx);
                        }
                    }
                    let voice_active = if vad_disabled {
                        true
                    } else {
                        vad.observe(input_buf) == VadDecision::Send
                    };
                    if !loop_guard.is_paused() && voice_active {
                        let frame = PcmFrame::new(input_buf.to_vec(), input_rate);
                        let frame16 = up_resampler.process(&frame);
                        if enqueue_latest_upstream(&audio_tx, &mut pending_upstream, frame16).is_err() {
                            return;
                        }
                    }
                }
                if last_diag_log.elapsed() >= DIAGNOSTIC_LOG_INTERVAL {
                    log_link_diagnostics(
                        kind,
                        output_rate,
                        pending_downstream.len(),
                        downstream_dropped_samples,
                        &loop_guard,
                    );
                    last_diag_log = Instant::now();
                }
            }
            maybe_frame = audio_rx.recv() => {
                let Some(frame24) = maybe_frame else {
                    return;
                };
                if loop_guard.is_paused() {
                    downstream_dropped_samples += frame24.samples.len() as u64;
                    continue;
                }
                for chunk in frame24.samples.chunks(DEVICE_FRAME) {
                    let mut block = chunk.to_vec();
                    block.resize(DEVICE_FRAME, 0);
                    let frame = PcmFrame::new(block, GEMINI_OUT_RATE);
                    let out = down_resampler.process(&frame);
                    if loopguard_enabled {
                        loop_guard.observe_injected(&out.samples);
                    }
                    downstream_dropped_samples += append_bounded_downstream(
                        &mut pending_downstream,
                        &out.samples,
                        max_pending_downstream,
                    ) as u64;
                }
                flush_downstream(output, &mut pending_downstream);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopPipelineAction {
    Pause,
    Resume,
}

#[derive(Debug)]
struct LoopGuardRuntime {
    thresholds: LoopThresholds,
    state: LoopGuardState,
    injected: Vec<FrameEnergy>,
    captured: Vec<FrameEnergy>,
    window_frames: usize,
    last_evidence: LoopEvidence,
}

impl LoopGuardRuntime {
    fn new(thresholds: LoopThresholds, window_frames: usize) -> Self {
        Self {
            thresholds,
            state: LoopGuardState::Clear,
            injected: Vec::with_capacity(window_frames),
            captured: Vec::with_capacity(window_frames),
            window_frames,
            last_evidence: LoopEvidence {
                suspected: false,
                lag_frames: 0,
                xcorr: 0.0,
                ratio_db: f32::NEG_INFINITY,
            },
        }
    }

    fn observe_injected(&mut self, samples: &[i16]) {
        push_energy_window(
            &mut self.injected,
            frame_energy(samples),
            self.window_frames,
        );
    }

    fn observe_captured(&mut self, samples: &[i16]) -> Option<LoopPipelineAction> {
        push_energy_window(
            &mut self.captured,
            frame_energy(samples),
            self.window_frames,
        );
        if self.injected.len() < 4 || self.captured.len() < 4 {
            if self.is_paused() {
                self.last_evidence = LoopEvidence {
                    suspected: false,
                    lag_frames: 0,
                    xcorr: 0.0,
                    ratio_db: f32::NEG_INFINITY,
                };
                return self.step();
            }
            return None;
        }
        self.last_evidence = detect_loop(&self.injected, &self.captured, &self.thresholds);
        self.step()
    }

    fn step(&mut self) -> Option<LoopPipelineAction> {
        let (next, action) = step_guard(self.state, &self.last_evidence, &self.thresholds);
        self.state = next;
        match action {
            Some(GuardAction::Pause) => {
                self.injected.clear();
                self.captured.clear();
                Some(LoopPipelineAction::Pause)
            }
            Some(GuardAction::Resume) => {
                self.injected.clear();
                self.captured.clear();
                Some(LoopPipelineAction::Resume)
            }
            None => None,
        }
    }

    fn is_paused(&self) -> bool {
        matches!(self.state, LoopGuardState::Paused { .. })
    }

    fn last_evidence(&self) -> LoopEvidence {
        self.last_evidence
    }
}

fn push_energy_window(window: &mut Vec<FrameEnergy>, energy: FrameEnergy, max_len: usize) {
    if max_len == 0 {
        return;
    }
    window.push(energy);
    if window.len() > max_len {
        let excess = window.len() - max_len;
        window.drain(0..excess);
    }
}

fn conservative_loop_thresholds() -> LoopThresholds {
    LoopThresholds {
        energy_ratio_db: -12.0,
        min_xcorr: 0.85,
        max_lag_frames: 40,
        hold_frames: 40,
        release_frames: 80,
    }
}

fn handle_loop_action(
    action: LoopPipelineAction,
    kind: LinkKind,
    guard: &LoopGuardRuntime,
    state_tx: &watch::Sender<SessionState>,
    evt_tx: &mpsc::Sender<ControlEvent>,
) {
    match action {
        LoopPipelineAction::Pause => {
            let ev = guard.last_evidence();
            tracing::warn!(
                ?kind,
                lag_frames = ev.lag_frames,
                xcorr = ev.xcorr,
                ratio_db = ev.ratio_db,
                "检测到疑似声学/应用层回环，自动暂停翻译链路"
            );
            let _ = state_tx.send(SessionState::Paused);
            send_control_event(
                evt_tx,
                ControlEvent::LoopSuspected {
                    lag_frames: ev.lag_frames,
                    xcorr: ev.xcorr,
                },
            );
            send_control_event(
                evt_tx,
                ControlEvent::TranslationPaused {
                    reason: PauseReason::AcousticLoop,
                },
            );
        }
        LoopPipelineAction::Resume => {
            tracing::info!(?kind, "回环检测恢复清白，恢复翻译链路");
            let _ = state_tx.send(SessionState::Running);
            send_control_event(evt_tx, ControlEvent::TranslationResumed);
        }
    }
}

fn send_control_event(evt_tx: &mpsc::Sender<ControlEvent>, event: ControlEvent) {
    if let Err(err) = evt_tx.try_send(event) {
        tracing::warn!(error = %err, "控制事件队列已满或关闭，丢弃事件");
    }
}

fn append_bounded_downstream(pending: &mut Vec<i16>, samples: &[i16], max_samples: usize) -> usize {
    pending.extend_from_slice(samples);
    if max_samples == 0 {
        let dropped = pending.len();
        pending.clear();
        return dropped;
    }
    if pending.len() <= max_samples {
        return 0;
    }
    let dropped = pending.len() - max_samples;
    pending.drain(0..dropped);
    dropped
}

fn flush_downstream(output: &mut audio_core::AudioProducer, pending: &mut Vec<i16>) -> usize {
    if pending.is_empty() {
        return 0;
    }
    let written = output.push_slice(pending);
    if written > 0 {
        pending.drain(0..written);
    }
    written
}

fn samples_for_ms(rate: u32, ms: usize) -> usize {
    (rate as usize * ms / 1_000).max(1)
}

fn log_link_diagnostics(
    kind: LinkKind,
    output_rate: u32,
    pending_downstream_samples: usize,
    downstream_dropped_samples: u64,
    loop_guard: &LoopGuardRuntime,
) {
    let pending_ms = pending_downstream_samples as f64 * 1_000.0 / output_rate as f64;
    let dropped_ms = downstream_dropped_samples as f64 * 1_000.0 / output_rate as f64;
    let proxy_ms = pending_ms + DOWNSTREAM_PENDING_MS as f64;
    let ev = loop_guard.last_evidence();
    tracing::info!(
        ?kind,
        pending_downstream_ms = pending_ms,
        latency_proxy_ms = proxy_ms,
        downstream_dropped_samples,
        downstream_dropped_ms = dropped_ms,
        loop_xcorr = ev.xcorr,
        loop_lag_frames = ev.lag_frames,
        loop_ratio_db = ev.ratio_db,
        loop_paused = loop_guard.is_paused(),
        "链路低频诊断"
    );
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

    #[test]
    fn downstream_pending_drops_oldest_to_bound_extra_latency() {
        let mut pending = vec![1, 2, 3, 4, 5, 6];
        let dropped = append_bounded_downstream(&mut pending, &[7, 8, 9, 10], 6);

        assert_eq!(dropped, 4);
        assert_eq!(pending, vec![5, 6, 7, 8, 9, 10]);
    }

    #[test]
    fn loop_guard_runtime_pauses_and_resumes_with_events() {
        let thresholds = diagnostics::LoopThresholds {
            hold_frames: 2,
            release_frames: 2,
            min_xcorr: 0.8,
            energy_ratio_db: -12.0,
            max_lag_frames: 6,
        };
        let mut guard = LoopGuardRuntime::new(thresholds, 16);
        let mut actions = Vec::new();

        for n in 0..18 {
            let injected = vec![((n * 97) % 5000 + 500) as i16; DEVICE_FRAME];
            guard.observe_injected(&injected);
            let captured = if n >= 3 {
                vec![(((n - 3) * 97) % 5000 + 500) as i16; DEVICE_FRAME]
            } else {
                vec![0; DEVICE_FRAME]
            };
            if let Some(action) = guard.observe_captured(&captured) {
                actions.push(action);
                break;
            }
        }

        assert_eq!(actions, vec![LoopPipelineAction::Pause]);
        assert!(guard.is_paused());

        for _ in 0..2 {
            if let Some(action) = guard.observe_captured(&[0; DEVICE_FRAME]) {
                actions.push(action);
            }
        }

        assert_eq!(
            actions,
            vec![LoopPipelineAction::Pause, LoopPipelineAction::Resume]
        );
        assert!(!guard.is_paused());
    }

    fn frame(sample: i16) -> PcmFrame {
        PcmFrame::new(vec![sample; 160], GEMINI_IN_RATE)
    }
}
