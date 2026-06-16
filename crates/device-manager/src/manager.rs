//! DeviceManager：持有不可变 current 快照，提供 refresh/查询。不在数据面热路径。
use crate::classify::{classify, DeviceUse};
use crate::snapshot::DeviceSnapshot;
use audio_core::{AudioBackend, AudioError, DeviceId, DeviceInfo, Direction};

pub struct DeviceManager<B: AudioBackend> {
    backend: B,
    current: DeviceSnapshot,
}

impl<B: AudioBackend> DeviceManager<B> {
    pub fn new(backend: B) -> Result<Self, AudioError> {
        let current = DeviceSnapshot {
            inputs: backend.list_inputs()?,
            outputs: backend.list_outputs()?,
        };
        Ok(Self { backend, current })
    }

    /// 重新枚举，返回新快照（不原地 mutate 旧快照内容，整体替换为新 Vec）。
    pub fn refresh(&mut self) -> Result<DeviceSnapshot, AudioError> {
        let next = DeviceSnapshot {
            inputs: self.backend.list_inputs()?,
            outputs: self.backend.list_outputs()?,
        };
        self.current = next.clone();
        Ok(next)
    }

    pub fn snapshot(&self) -> &DeviceSnapshot {
        &self.current
    }

    pub fn default_for(&self, dir: Direction) -> Option<&DeviceInfo> {
        let list = match dir {
            Direction::Input => &self.current.inputs,
            Direction::Output => &self.current.outputs,
        };
        list.iter().find(|d| d.is_default)
    }

    pub fn pick_use(&self, u: DeviceUse) -> Vec<&DeviceInfo> {
        let mut out = Vec::new();
        for d in &self.current.inputs {
            if classify(d, Direction::Input) == u {
                out.push(d);
            }
        }
        for d in &self.current.outputs {
            if classify(d, Direction::Output) == u {
                out.push(d);
            }
        }
        out
    }

    pub fn resolve(&self, id: &DeviceId, dir: Direction) -> Option<&DeviceInfo> {
        let list = match dir {
            Direction::Input => &self.current.inputs,
            Direction::Output => &self.current.outputs,
        };
        list.iter().find(|d| &d.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_core::{
        AudioConsumer, AudioProducer, DeviceWatchHandle, InputStream, OutputStream, StreamCfg,
    };
    use std::cell::RefCell;

    fn d(name: &str, def: bool, virt: bool) -> DeviceInfo {
        DeviceInfo {
            id: DeviceId(name.into()),
            name: name.into(),
            is_default: def,
            is_virtual: virt,
        }
    }

    struct MockBackend {
        inputs: RefCell<Vec<DeviceInfo>>,
        outputs: RefCell<Vec<DeviceInfo>>,
    }

    // SAFETY: 测试单线程使用；AudioBackend 要求 Send+Sync，RefCell 在测试上下文不跨线程。
    unsafe impl Sync for MockBackend {}

    impl AudioBackend for MockBackend {
        fn list_inputs(&self) -> Result<Vec<DeviceInfo>, AudioError> {
            Ok(self.inputs.borrow().clone())
        }

        fn list_outputs(&self) -> Result<Vec<DeviceInfo>, AudioError> {
            Ok(self.outputs.borrow().clone())
        }

        fn open_input(
            &self,
            _: &DeviceId,
            _: StreamCfg,
        ) -> Result<(Box<dyn InputStream>, AudioConsumer), AudioError> {
            Err(AudioError::OpenStream("mock 不打开输入流".into()))
        }

        fn open_output(
            &self,
            _: &DeviceId,
            _: StreamCfg,
        ) -> Result<(Box<dyn OutputStream>, AudioProducer), AudioError> {
            Err(AudioError::OpenStream("mock 不打开输出流".into()))
        }

        fn watch_devices(&self) -> Result<DeviceWatchHandle, AudioError> {
            let (_tx, rx) = std::sync::mpsc::channel();
            Ok(DeviceWatchHandle::new(rx))
        }
    }

    #[test]
    fn refresh_returns_new_immutable_snapshot() {
        let backend = MockBackend {
            inputs: RefCell::new(vec![d("MicX", true, false)]),
            outputs: RefCell::new(vec![d("BlackHole 2ch", false, true)]),
        };
        let mut mgr = DeviceManager::new(backend).unwrap();
        let old = mgr.snapshot().clone();
        assert_eq!(old.inputs.len(), 1);

        mgr.backend
            .inputs
            .borrow_mut()
            .push(d("BlackHole 16ch", false, true));
        let new = mgr.refresh().unwrap();
        assert_ne!(old, new, "refresh 必须产生新快照，未原地 mutate");

        let events = crate::snapshot::diff_snapshots(&old, &new);
        assert!(events.iter().any(|e| matches!(
            e,
            crate::watch::DeviceEvent::Added {
                dir: Direction::Input,
                ..
            }
        )));
        assert_eq!(
            classify(&new.inputs[1], Direction::Input),
            DeviceUse::VirtualSpeaker
        );
    }
}
