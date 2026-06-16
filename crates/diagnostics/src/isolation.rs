//! 第一道防线：启动期结构性隔离。
use audio_core::DeviceId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRoute {
    pub source: DeviceId,
    pub sink: DeviceId,
    pub source_is_virtual: bool,
    pub sink_is_virtual: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IsolationError {
    SourceSinkOverlap { device: String },
    OutputIsVirtualCaptureSource { device: String },
}

pub fn validate_isolation(links: &[LinkRoute]) -> Result<(), IsolationError> {
    let sources: Vec<&DeviceId> = links.iter().map(|link| &link.source).collect();
    for link in links {
        if sources.iter().any(|source| **source == link.sink) {
            let sink_used_as_virtual_source = links
                .iter()
                .any(|other| other.source == link.sink && other.source_is_virtual);
            if link.sink_is_virtual && sink_used_as_virtual_source {
                return Err(IsolationError::OutputIsVirtualCaptureSource {
                    device: link.sink.0.clone(),
                });
            }
            return Err(IsolationError::SourceSinkOverlap {
                device: link.sink.0.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> DeviceId {
        DeviceId(s.into())
    }

    #[test]
    fn isolation_rejects_same_device_as_source_and_sink() {
        let bad = [LinkRoute {
            source: id("BlackHole"),
            sink: id("BlackHole"),
            source_is_virtual: true,
            sink_is_virtual: true,
        }];
        assert!(matches!(
            validate_isolation(&bad),
            Err(IsolationError::OutputIsVirtualCaptureSource { .. })
        ));

        let ok = [
            LinkRoute {
                source: id("PhysMic"),
                sink: id("VirtMic"),
                source_is_virtual: false,
                sink_is_virtual: true,
            },
            LinkRoute {
                source: id("VirtSpk"),
                sink: id("PhysHeadset"),
                source_is_virtual: true,
                sink_is_virtual: false,
            },
        ];
        assert_eq!(validate_isolation(&ok), Ok(()));
    }

    #[test]
    fn isolation_rejects_output_to_virtual_capture_source() {
        let bad = [
            LinkRoute {
                source: id("PhysMic"),
                sink: id("VirtSpk"),
                source_is_virtual: false,
                sink_is_virtual: true,
            },
            LinkRoute {
                source: id("VirtSpk"),
                sink: id("PhysHeadset"),
                source_is_virtual: true,
                sink_is_virtual: false,
            },
        ];
        assert!(matches!(
            validate_isolation(&bad),
            Err(IsolationError::OutputIsVirtualCaptureSource { .. })
        ));
    }
}
