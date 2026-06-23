//! 顶层编排：持 RouteMatrix 与 0..2 LinkHandle，聚合两 watch，子状态变更即发事件。
use crate::control::{worst_state, ControlEvent, SessionState};
use crate::link::{spawn_link, LinkHandle};
use crate::route::{RouteError, RouteMatrix};
use audio_core::AudioBackend;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct Orchestrator {
    matrix: RouteMatrix,
    uplink: Option<LinkHandle>,
    downlink: Option<LinkHandle>,
}

impl Orchestrator {
    pub fn top_state(&self) -> SessionState {
        let up = self
            .uplink
            .as_ref()
            .map(|handle| handle.current_state())
            .unwrap_or(SessionState::Idle);
        let down = self
            .downlink
            .as_ref()
            .map(|handle| handle.current_state())
            .unwrap_or(SessionState::Idle);
        worst_state(&up, &down)
    }

    pub fn route_matrix(&self) -> &RouteMatrix {
        &self.matrix
    }

    pub fn stop(self) {
        if let Some(handle) = &self.uplink {
            handle.abort();
        }
        if let Some(handle) = &self.downlink {
            handle.abort();
        }
    }
}

pub async fn start(
    matrix: RouteMatrix,
    backend: Arc<dyn AudioBackend>,
    make_url: Arc<dyn Fn() -> String + Send + Sync>,
    evt_tx: mpsc::Sender<ControlEvent>,
) -> Result<Orchestrator, RouteError> {
    let mut uplink = None;
    let mut downlink = None;
    if let Some(role) = &matrix.uplink {
        uplink = Some(spawn_link(role, backend.clone(), make_url.clone(), evt_tx.clone()).await?);
    }
    if let Some(role) = &matrix.downlink {
        downlink = Some(spawn_link(role, backend.clone(), make_url.clone(), evt_tx.clone()).await?);
    }
    spawn_state_relay(&uplink, &downlink, evt_tx);
    Ok(Orchestrator {
        matrix,
        uplink,
        downlink,
    })
}

fn spawn_state_relay(
    up: &Option<LinkHandle>,
    down: &Option<LinkHandle>,
    evt_tx: mpsc::Sender<ControlEvent>,
) {
    if let Some(handle) = up {
        let mut rx = handle.state.clone();
        let tx = evt_tx.clone();
        tokio::spawn(async move {
            loop {
                let state = rx.borrow().clone();
                if tx.send(ControlEvent::UplinkState(state)).await.is_err() {
                    break;
                }
                if rx.changed().await.is_err() {
                    break;
                }
            }
        });
    }
    if let Some(handle) = down {
        let mut rx = handle.state.clone();
        let tx = evt_tx;
        tokio::spawn(async move {
            loop {
                let state = rx.borrow().clone();
                if tx.send(ControlEvent::DownlinkState(state)).await.is_err() {
                    break;
                }
                if rx.changed().await.is_err() {
                    break;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::{LinkKind, LinkRole};
    use audio_core::DeviceId;
    use tokio::sync::watch;

    fn handle(kind: LinkKind, init: SessionState) -> (LinkHandle, watch::Sender<SessionState>) {
        let (tx, rx) = watch::channel(init);
        let abort = tokio::spawn(async { std::future::pending::<()>().await }).abort_handle();
        (
            LinkHandle {
                kind,
                state: rx,
                abort,
            },
            tx,
        )
    }

    fn role(kind: LinkKind) -> LinkRole {
        LinkRole {
            kind,
            target_lang: "en".into(),
            source: crate::control::SourceLang::Auto,
            in_dev: DeviceId("a".into()),
            out_dev: DeviceId("b".into()),
        }
    }

    #[tokio::test]
    async fn orchestrator_projects_worst_state_per_link() {
        let (up_handle, _up_tx) = handle(LinkKind::Uplink, SessionState::Running);
        let (down_handle, _down_tx) = handle(
            LinkKind::Downlink,
            SessionState::Reconnecting { attempt: 2 },
        );
        let (evt_tx, mut evt_rx) = mpsc::channel(8);
        let matrix = RouteMatrix {
            uplink: Some(role(LinkKind::Uplink)),
            downlink: Some(role(LinkKind::Downlink)),
        };
        let orch = Orchestrator {
            matrix,
            uplink: Some(up_handle),
            downlink: Some(down_handle),
        };
        assert_eq!(
            orch.top_state(),
            worst_state(
                &SessionState::Running,
                &SessionState::Reconnecting { attempt: 2 }
            )
        );

        super::spawn_state_relay(&orch.uplink, &orch.downlink, evt_tx);
        let mut got = Vec::new();
        for _ in 0..2 {
            if let Ok(Some(event)) =
                tokio::time::timeout(std::time::Duration::from_millis(500), evt_rx.recv()).await
            {
                got.push(event);
            }
        }
        assert!(got.contains(&ControlEvent::UplinkState(SessionState::Running)));
        assert!(
            got.contains(&ControlEvent::DownlinkState(SessionState::Reconnecting {
                attempt: 2
            }))
        );
    }
}
