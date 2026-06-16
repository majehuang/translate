//! 设备快照与前后 diff。快照不可变：刷新产生新 Vec，绝不原地 mutate。
use crate::watch::DeviceEvent;
use audio_core::{DeviceId, DeviceInfo, Direction};

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceSnapshot {
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
}

impl DeviceSnapshot {
    pub fn contains(&self, id: &DeviceId, dir: Direction) -> bool {
        let list = match dir {
            Direction::Input => &self.inputs,
            Direction::Output => &self.outputs,
        };
        list.iter().any(|d| &d.id == id)
    }

    fn default_of(list: &[DeviceInfo]) -> Option<&DeviceId> {
        list.iter().find(|d| d.is_default).map(|d| &d.id)
    }
}

/// 前后快照 diff：Added/Removed/DefaultChanged。纯函数，不 mutate 入参。
pub fn diff_snapshots(prev: &DeviceSnapshot, next: &DeviceSnapshot) -> Vec<DeviceEvent> {
    let mut out = Vec::new();
    for (dir, previous, current) in [
        (Direction::Input, &prev.inputs, &next.inputs),
        (Direction::Output, &prev.outputs, &next.outputs),
    ] {
        for device in current {
            if !previous.iter().any(|old| old.id == device.id) {
                out.push(DeviceEvent::Added {
                    id: device.id.clone(),
                    dir,
                });
            }
        }
        for device in previous {
            if !current.iter().any(|new| new.id == device.id) {
                out.push(DeviceEvent::Removed {
                    id: device.id.clone(),
                    dir,
                });
            }
        }
        let previous_default = DeviceSnapshot::default_of(previous);
        let current_default = DeviceSnapshot::default_of(current);
        if previous_default != current_default {
            if let Some(new) = current_default {
                out.push(DeviceEvent::DefaultChanged {
                    dir,
                    new: new.clone(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::project_lost;

    fn d(name: &str, def: bool) -> DeviceInfo {
        DeviceInfo {
            id: DeviceId(name.into()),
            name: name.into(),
            is_default: def,
            is_virtual: false,
        }
    }

    fn snap(ins: Vec<DeviceInfo>, outs: Vec<DeviceInfo>) -> DeviceSnapshot {
        DeviceSnapshot {
            inputs: ins,
            outputs: outs,
        }
    }

    fn sorted(mut v: Vec<DeviceEvent>) -> Vec<DeviceEvent> {
        v.sort_by_key(|e| format!("{e:?}"));
        v
    }

    #[test]
    fn diff_added_and_removed_set() {
        let prev = snap(vec![d("A", false), d("B", false)], vec![]);
        let next = snap(vec![d("B", false), d("C", false)], vec![]);
        let got = sorted(diff_snapshots(&prev, &next));
        assert_eq!(
            got,
            sorted(vec![
                DeviceEvent::Added {
                    id: DeviceId("C".into()),
                    dir: Direction::Input
                },
                DeviceEvent::Removed {
                    id: DeviceId("A".into()),
                    dir: Direction::Input
                },
            ])
        );
    }

    #[test]
    fn diff_default_change_only() {
        let prev = snap(vec![d("A", true), d("B", false)], vec![]);
        let next = snap(vec![d("A", false), d("B", true)], vec![]);
        assert_eq!(
            diff_snapshots(&prev, &next),
            vec![DeviceEvent::DefaultChanged {
                dir: Direction::Input,
                new: DeviceId("B".into())
            }]
        );
    }

    #[test]
    fn project_lost_only_for_in_use() {
        let next = snap(vec![d("SpkY", false)], vec![]);
        let in_use = vec![
            (DeviceId("MicX".into()), Direction::Input),
            (DeviceId("SpkY".into()), Direction::Input),
        ];
        assert_eq!(
            project_lost(&in_use, &next),
            vec![DeviceEvent::DeviceLost {
                id: DeviceId("MicX".into()),
                dir: Direction::Input
            }]
        );
        let still = vec![(DeviceId("SpkY".into()), Direction::Input)];
        assert!(project_lost(&still, &next).is_empty());
    }
}
