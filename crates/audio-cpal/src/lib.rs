//! CPAL 实现的 AudioBackend，覆盖 Windows(WASAPI) 与 macOS(CoreAudio)。
use audio_core::{
    audio_channel, is_virtual_device_name, AudioBackend, AudioConsumer, AudioError, AudioProducer,
    DeviceId, DeviceInfo, InputStream, OutputStream, StreamCfg,
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::sync::mpsc;
use std::time::Duration;

pub struct CpalBackend {
    host: cpal::Host,
}

impl CpalBackend {
    pub fn new() -> Self {
        Self {
            host: cpal::default_host(),
        }
    }

    fn find_input(&self, id: &DeviceId) -> Result<cpal::Device, AudioError> {
        self.host
            .input_devices()
            .map_err(|e| AudioError::OpenStream(e.to_string()))?
            .find(|device| device.name().map(|name| name == id.0).unwrap_or(false))
            .ok_or_else(|| AudioError::DeviceNotFound(id.0.clone()))
    }

    fn find_output(&self, id: &DeviceId) -> Result<cpal::Device, AudioError> {
        self.host
            .output_devices()
            .map_err(|e| AudioError::OpenStream(e.to_string()))?
            .find(|device| device.name().map(|name| name == id.0).unwrap_or(false))
            .ok_or_else(|| AudioError::DeviceNotFound(id.0.clone()))
    }
}

impl Default for CpalBackend {
    fn default() -> Self {
        Self::new()
    }
}

struct CpalInput {
    _stream: cpal::Stream,
    rate: u32,
}

// CPAL Stream 在平台音频线程驱动；句柄在此仅用于保活。
unsafe impl Send for CpalInput {}

impl InputStream for CpalInput {
    fn actual_sample_rate(&self) -> u32 {
        self.rate
    }
}

struct CpalOutput {
    _stream: cpal::Stream,
    rate: u32,
}

unsafe impl Send for CpalOutput {}

impl OutputStream for CpalOutput {
    fn actual_sample_rate(&self) -> u32 {
        self.rate
    }
}

fn to_info(device: &cpal::Device, default_name: &Option<String>) -> DeviceInfo {
    let name = device.name().unwrap_or_else(|_| "<unknown>".into());
    let is_default = default_name.as_deref() == Some(name.as_str());
    DeviceInfo {
        is_virtual: is_virtual_device_name(&name),
        is_default,
        id: DeviceId(name.clone()),
        name,
    }
}

fn list_with_timeout(
    direction: &'static str,
    list: impl FnOnce(&cpal::Host) -> Result<Vec<DeviceInfo>, AudioError> + Send + 'static,
) -> Result<Vec<DeviceInfo>, AudioError> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let host = cpal::default_host();
        let _ = tx.send(list(&host));
    });
    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            tracing::warn!(direction, "CPAL 设备枚举超时，返回空列表");
            Ok(Vec::new())
        }
        Err(err) => Err(AudioError::OpenStream(err.to_string())),
    }
}

fn sample_to_i16<T>(sample: T) -> i16
where
    T: Sample,
    i16: FromSample<T>,
{
    i16::from_sample(sample)
}

fn sample_from_i16<T>(sample: i16) -> T
where
    T: Sample + FromSample<i16>,
{
    T::from_sample(sample)
}

fn build_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    mut prod: AudioProducer,
) -> Result<cpal::Stream, AudioError>
where
    T: Sample + SizedSample,
    i16: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                for frame in data.chunks(channels) {
                    if let Some(sample) = frame.first() {
                        let mono = sample_to_i16(*sample);
                        let _ = prod.push_slice(&[mono]);
                    }
                }
            },
            |err| tracing::error!(?err, "输入流错误"),
            None,
        )
        .map_err(|e| AudioError::OpenStream(e.to_string()))
}

fn build_output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    mut cons: AudioConsumer,
) -> Result<cpal::Stream, AudioError>
where
    T: Sample + SizedSample + FromSample<i16>,
{
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                for frame in data.chunks_mut(channels) {
                    let mut mono = [0i16; 1];
                    let got = cons.pop_slice(&mut mono);
                    let sample = if got == 1 { mono[0] } else { 0 };
                    let value = sample_from_i16::<T>(sample);
                    for channel in frame {
                        *channel = value;
                    }
                }
            },
            |err| tracing::error!(?err, "输出流错误"),
            None,
        )
        .map_err(|e| AudioError::OpenStream(e.to_string()))
}

impl AudioBackend for CpalBackend {
    fn list_inputs(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        list_with_timeout("input", |host| {
            let default = host
                .default_input_device()
                .and_then(|device| device.name().ok());
            Ok(host
                .input_devices()
                .map_err(|e| AudioError::OpenStream(e.to_string()))?
                .map(|device| to_info(&device, &default))
                .collect())
        })
    }

    fn list_outputs(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        list_with_timeout("output", |host| {
            let default = host
                .default_output_device()
                .and_then(|device| device.name().ok());
            Ok(host
                .output_devices()
                .map_err(|e| AudioError::OpenStream(e.to_string()))?
                .map(|device| to_info(&device, &default))
                .collect())
        })
    }

    fn open_input(
        &self,
        id: &DeviceId,
        cfg: StreamCfg,
    ) -> Result<(Box<dyn InputStream>, AudioConsumer), AudioError> {
        let device = self.find_input(id)?;
        let supported = device
            .default_input_config()
            .map_err(|e| AudioError::OpenStream(e.to_string()))?;
        let rate = supported.sample_rate().0;
        let channels = supported.channels() as usize;
        let (prod, cons) = audio_channel(rate as usize * channels);
        let stream_config = supported.config();
        let stream = match supported.sample_format() {
            SampleFormat::F32 => build_input_stream::<f32>(&device, &stream_config, channels, prod),
            SampleFormat::I16 => build_input_stream::<i16>(&device, &stream_config, channels, prod),
            SampleFormat::U16 => build_input_stream::<u16>(&device, &stream_config, channels, prod),
            sample_format => Err(AudioError::OpenStream(format!(
                "不支持的输入采样格式: {sample_format:?}"
            ))),
        }?;
        stream
            .play()
            .map_err(|e| AudioError::OpenStream(e.to_string()))?;
        let _ = cfg;
        Ok((
            Box::new(CpalInput {
                _stream: stream,
                rate,
            }),
            cons,
        ))
    }

    fn open_output(
        &self,
        id: &DeviceId,
        cfg: StreamCfg,
    ) -> Result<(Box<dyn OutputStream>, AudioProducer), AudioError> {
        let device = self.find_output(id)?;
        let supported = device
            .default_output_config()
            .map_err(|e| AudioError::OpenStream(e.to_string()))?;
        let rate = supported.sample_rate().0;
        let channels = supported.channels() as usize;
        let (prod, cons) = audio_channel(rate as usize * channels);
        let stream_config = supported.config();
        let stream = match supported.sample_format() {
            SampleFormat::F32 => build_output_stream::<f32>(&device, &stream_config, channels, cons),
            SampleFormat::I16 => build_output_stream::<i16>(&device, &stream_config, channels, cons),
            SampleFormat::U16 => build_output_stream::<u16>(&device, &stream_config, channels, cons),
            sample_format => Err(AudioError::OpenStream(format!(
                "不支持的输出采样格式: {sample_format:?}"
            ))),
        }?;
        stream
            .play()
            .map_err(|e| AudioError::OpenStream(e.to_string()))?;
        let _ = cfg;
        Ok((
            Box::new(CpalOutput {
                _stream: stream,
                rate,
            }),
            prod,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_core::AudioBackend;

    #[test]
    fn backend_lists_without_panicking() {
        let backend = CpalBackend::new();
        let _ = backend.list_inputs();
        let _ = backend.list_outputs();
    }
}
