//! 控制面：低频、消息驱动。与数据面（环形缓冲）严格分离。

/// 翻译方向模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslateMode {
    Bidirectional,
    UplinkOnly,
    DownlinkOnly,
}

/// 源语言配置：auto 模式不锁源、由模型自动识别。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceLang {
    Locked(String), // BCP-47, 如 "zh"
    Auto,
}

/// 单条 Session 的运行子状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Starting,
    Running,
    Reconnecting { attempt: u32 },
    Error(String),
}

/// 控制面发给 UI 的事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlEvent {
    UplinkState(SessionState),
    DownlinkState(SessionState),
    /// auto 模式下模型返回的检测语言码（只读，供 UI 显示/字幕预埋）。
    DetectedLanguage {
        uplink: bool,
        code: String,
    },
}

/// 将上下行子状态投影为 UI 顶层状态：取“最坏”。
pub fn worst_state(up: &SessionState, down: &SessionState) -> SessionState {
    fn rank(s: &SessionState) -> u8 {
        match s {
            SessionState::Error(_) => 4,
            SessionState::Reconnecting { .. } => 3,
            SessionState::Starting => 2,
            SessionState::Running => 1,
            SessionState::Idle => 0,
        }
    }
    if rank(up) >= rank(down) {
        up.clone()
    } else {
        down.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worst_picks_error_over_running() {
        let up = SessionState::Running;
        let down = SessionState::Error("net".into());
        assert_eq!(worst_state(&up, &down), SessionState::Error("net".into()));
    }

    #[test]
    fn worst_picks_reconnecting_over_running() {
        let up = SessionState::Reconnecting { attempt: 2 };
        let down = SessionState::Running;
        assert_eq!(
            worst_state(&up, &down),
            SessionState::Reconnecting { attempt: 2 }
        );
    }

    #[test]
    fn both_running_is_running() {
        assert_eq!(
            worst_state(&SessionState::Running, &SessionState::Running),
            SessionState::Running
        );
    }
}
