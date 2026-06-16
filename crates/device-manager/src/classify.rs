//! 设备用途分类：用途由方向与虚拟设备标记共同决定。
use audio_core::{DeviceInfo, Direction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceUse {
    VirtualMic,
    VirtualSpeaker,
    Physical,
}

pub fn classify(info: &DeviceInfo, dir: Direction) -> DeviceUse {
    match (info.is_virtual, dir) {
        (true, Direction::Output) => DeviceUse::VirtualMic,
        (true, Direction::Input) => DeviceUse::VirtualSpeaker,
        (false, _) => DeviceUse::Physical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_core::DeviceId;

    fn dev(name: &str, virt: bool) -> DeviceInfo {
        DeviceInfo {
            id: DeviceId(name.into()),
            name: name.into(),
            is_default: false,
            is_virtual: virt,
        }
    }

    #[test]
    fn classify_uses_direction_plus_virtual_name() {
        assert_eq!(
            classify(
                &dev("CABLE Input (VB-Audio Virtual Cable)", true),
                Direction::Output
            ),
            DeviceUse::VirtualMic
        );
        assert_eq!(
            classify(&dev("BlackHole 2ch", true), Direction::Input),
            DeviceUse::VirtualSpeaker
        );
        assert_eq!(
            classify(&dev("MacBook Pro Microphone", false), Direction::Input),
            DeviceUse::Physical
        );
    }
}
