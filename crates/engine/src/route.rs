//! 路由矩阵：启动时一次性生成、运行中只读。平台无关纯函数。
use crate::control::{SourceLang, TranslateMode};
use audio_core::{DeviceId, DeviceInfo};
use diagnostics::{validate_isolation as diag_validate_isolation, IsolationError, LinkRoute};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Uplink,
    Downlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRole {
    pub kind: LinkKind,
    pub target_lang: String,
    pub source: SourceLang,
    pub in_dev: DeviceId,
    pub out_dev: DeviceId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteMatrix {
    pub uplink: Option<LinkRole>,
    pub downlink: Option<LinkRole>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkIntent {
    pub in_dev: DeviceId,
    pub out_dev: DeviceId,
    pub target_lang: String,
    pub source: SourceLang,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSpec {
    pub mode: TranslateMode,
    pub uplink: LinkIntent,
    pub downlink: LinkIntent,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RouteError {
    #[error("设备未找到: {0}")]
    DeviceNotFound(String),
    #[error("物理隔离冲突: 注入设备 {0} 与采集设备相同/重叠，拒绝启动")]
    SourceSinkOverlap(String),
    #[error("输出回流虚拟采集设备 {0}，可能形成自激环路")]
    VirtualLoopback(String),
}

impl From<IsolationError> for RouteError {
    fn from(error: IsolationError) -> Self {
        match error {
            IsolationError::SourceSinkOverlap { device } => RouteError::SourceSinkOverlap(device),
            IsolationError::OutputIsVirtualCaptureSource { device } => {
                RouteError::VirtualLoopback(device)
            }
        }
    }
}

pub fn active_links(mode: TranslateMode) -> (bool, bool) {
    match mode {
        TranslateMode::Bidirectional => (true, true),
        TranslateMode::UplinkOnly => (true, false),
        TranslateMode::DownlinkOnly => (false, true),
    }
}

fn find<'a>(devices: &'a [DeviceInfo], id: &DeviceId) -> Result<&'a DeviceInfo, RouteError> {
    devices
        .iter()
        .find(|device| &device.id == id)
        .ok_or_else(|| RouteError::DeviceNotFound(id.0.clone()))
}

pub fn build_routes(spec: &RouteSpec, devices: &[DeviceInfo]) -> Result<RouteMatrix, RouteError> {
    let (uplink_active, downlink_active) = active_links(spec.mode);
    let uplink = if uplink_active {
        find(devices, &spec.uplink.in_dev)?;
        find(devices, &spec.uplink.out_dev)?;
        Some(LinkRole {
            kind: LinkKind::Uplink,
            target_lang: spec.uplink.target_lang.clone(),
            source: spec.uplink.source.clone(),
            in_dev: spec.uplink.in_dev.clone(),
            out_dev: spec.uplink.out_dev.clone(),
        })
    } else {
        None
    };
    let downlink = if downlink_active {
        find(devices, &spec.downlink.in_dev)?;
        find(devices, &spec.downlink.out_dev)?;
        Some(LinkRole {
            kind: LinkKind::Downlink,
            target_lang: spec.downlink.target_lang.clone(),
            source: SourceLang::Auto,
            in_dev: spec.downlink.in_dev.clone(),
            out_dev: spec.downlink.out_dev.clone(),
        })
    } else {
        None
    };
    Ok(RouteMatrix { uplink, downlink })
}

pub fn validate_isolation(matrix: &RouteMatrix, devices: &[DeviceInfo]) -> Result<(), RouteError> {
    let mut links = Vec::new();
    for role in [matrix.uplink.as_ref(), matrix.downlink.as_ref()]
        .into_iter()
        .flatten()
    {
        let source = find(devices, &role.in_dev)?;
        let sink = find(devices, &role.out_dev)?;
        links.push(LinkRoute {
            source: role.in_dev.clone(),
            sink: role.out_dev.clone(),
            source_is_virtual: source.is_virtual,
            sink_is_virtual: sink.is_virtual,
        });
    }
    diag_validate_isolation(&links).map_err(RouteError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str, virt: bool) -> DeviceInfo {
        DeviceInfo {
            id: DeviceId(name.into()),
            name: name.into(),
            is_default: false,
            is_virtual: virt,
        }
    }

    fn devs() -> Vec<DeviceInfo> {
        vec![
            dev("PhysMic", false),
            dev("VirtMic", true),
            dev("VirtSpk", true),
            dev("PhysHeadset", false),
        ]
    }

    fn intent(input: &str, output: &str, target: &str) -> LinkIntent {
        LinkIntent {
            in_dev: DeviceId(input.into()),
            out_dev: DeviceId(output.into()),
            target_lang: target.into(),
            source: SourceLang::Auto,
        }
    }

    fn spec(mode: TranslateMode) -> RouteSpec {
        RouteSpec {
            mode,
            uplink: intent("PhysMic", "VirtMic", "en"),
            downlink: intent("VirtSpk", "PhysHeadset", "zh"),
        }
    }

    #[test]
    fn build_routes_uplink_only_drops_downlink() {
        let matrix = build_routes(&spec(TranslateMode::UplinkOnly), &devs()).unwrap();
        assert!(matrix.uplink.is_some() && matrix.downlink.is_none());
        assert_eq!(active_links(TranslateMode::UplinkOnly), (true, false));
        assert_eq!(active_links(TranslateMode::DownlinkOnly), (false, true));
        assert_eq!(active_links(TranslateMode::Bidirectional), (true, true));
    }

    #[test]
    fn build_routes_bidirectional_lights_two_links() {
        let matrix = build_routes(&spec(TranslateMode::Bidirectional), &devs()).unwrap();
        let up = matrix.uplink.unwrap();
        let down = matrix.downlink.unwrap();
        assert_eq!(up.target_lang, "en");
        assert_eq!(down.target_lang, "zh");
        assert_eq!(down.source, SourceLang::Auto);
        assert_ne!(up.in_dev, up.out_dev);
        assert_ne!(up.in_dev, down.in_dev);
        assert_ne!(up.out_dev, down.out_dev);
        assert_ne!(down.in_dev, down.out_dev);
    }

    #[test]
    fn validate_isolation_rejects_source_sink_overlap() {
        let mut route_spec = spec(TranslateMode::Bidirectional);
        route_spec.downlink.in_dev = DeviceId("VirtMic".into());
        let matrix = build_routes(&route_spec, &devs()).unwrap();
        assert!(matches!(
            validate_isolation(&matrix, &devs()),
            Err(RouteError::VirtualLoopback(_)) | Err(RouteError::SourceSinkOverlap(_))
        ));

        let ok = build_routes(&spec(TranslateMode::Bidirectional), &devs()).unwrap();
        assert_eq!(validate_isolation(&ok, &devs()), Ok(()));
    }

    #[test]
    fn validate_isolation_rejects_output_to_virtual_capture() {
        let mut route_spec = spec(TranslateMode::Bidirectional);
        route_spec.uplink.out_dev = DeviceId("VirtSpk".into());
        let matrix = build_routes(&route_spec, &devs()).unwrap();
        assert!(matches!(
            validate_isolation(&matrix, &devs()),
            Err(RouteError::VirtualLoopback(_))
        ));
    }

    #[test]
    fn build_routes_missing_device_errs() {
        let mut route_spec = spec(TranslateMode::Bidirectional);
        route_spec.uplink.in_dev = DeviceId("Ghost".into());
        assert!(matches!(
            build_routes(&route_spec, &devs()),
            Err(RouteError::DeviceNotFound(_))
        ));
    }
}
