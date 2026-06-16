//! M2 CLI 薄壳：设备分类列表、路由矩阵校验、双链路 engine 编排。
use audio_core::{AudioBackend, DeviceId, DeviceInfo, Direction};
use audio_cpal::CpalBackend;
use device_manager::{classify, DeviceManager, DeviceUse};
use engine::control::{ControlEvent, SourceLang, TranslateMode};
use engine::orchestrator;
use engine::route::{build_routes, validate_isolation, LinkIntent, RouteSpec};
use std::sync::Arc;
use tokio::sync::mpsc;

const GEMINI_WS: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let backend = CpalBackend::new();

    if arg_value(&args, "--uplink-in").is_none() {
        let manager = DeviceManager::new(backend)?;
        print_device_table(manager.snapshot());
        println!();
        println!("请用 --uplink-in/--uplink-out/--downlink-in/--downlink-out 指定设备后重跑。");
        return Ok(());
    }

    let spec = build_spec(&args)?;
    let devices = all_devices(&backend)?;
    let matrix = build_routes(&spec, &devices)?;
    validate_isolation(&matrix, &devices)?;

    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| anyhow::anyhow!("缺少 GEMINI_API_KEY 环境变量"))?;
    let make_url = Arc::new(move || format!("{GEMINI_WS}?key={api_key}"));
    let (evt_tx, mut evt_rx) = mpsc::channel::<ControlEvent>(64);
    let printer = tokio::spawn(async move {
        while let Some(event) = evt_rx.recv().await {
            println!("{event:?}");
        }
    });

    let orch = orchestrator::start(matrix, Arc::new(CpalBackend::new()), make_url, evt_tx).await?;
    println!("运行中，Ctrl+C 停止。当前顶层状态：{:?}", orch.top_state());
    tokio::signal::ctrl_c().await?;
    orch.stop();
    printer.abort();
    println!("已停止。");
    Ok(())
}

fn print_device_table(snapshot: &device_manager::DeviceSnapshot) {
    println!("== 输入设备 ==");
    for device in &snapshot.inputs {
        print_device(device, Direction::Input);
    }
    println!("== 输出设备 ==");
    for device in &snapshot.outputs {
        print_device(device, Direction::Output);
    }
}

fn print_device(device: &DeviceInfo, direction: Direction) {
    let default = if device.is_default { " [default]" } else { "" };
    let use_tag = match classify(device, direction) {
        DeviceUse::VirtualMic => "[virtual-mic]",
        DeviceUse::VirtualSpeaker => "[virtual-speaker]",
        DeviceUse::Physical => "[physical]",
    };
    println!("  {use_tag}{default} {}", device.name);
}

fn all_devices(backend: &CpalBackend) -> anyhow::Result<Vec<DeviceInfo>> {
    let mut devices = backend.list_inputs()?;
    devices.extend(backend.list_outputs()?);
    Ok(devices)
}

fn parse_mode(args: &[String]) -> TranslateMode {
    match arg_value(args, "--mode").as_deref() {
        Some("uplink-only") => TranslateMode::UplinkOnly,
        Some("downlink-only") => TranslateMode::DownlinkOnly,
        _ => TranslateMode::Bidirectional,
    }
}

fn build_spec(args: &[String]) -> anyhow::Result<RouteSpec> {
    let uplink_in = required_arg(args, "--uplink-in")?;
    let uplink_out = required_arg(args, "--uplink-out")?;
    let downlink_in = arg_value(args, "--downlink-in").unwrap_or_else(|| uplink_in.clone());
    let downlink_out = arg_value(args, "--downlink-out").unwrap_or_else(|| uplink_out.clone());
    let uplink_target = arg_value(args, "--uplink-target").unwrap_or_else(|| "en".into());
    let downlink_target = arg_value(args, "--downlink-target").unwrap_or_else(|| "zh".into());

    Ok(RouteSpec {
        mode: parse_mode(args),
        uplink: LinkIntent {
            in_dev: DeviceId(uplink_in),
            out_dev: DeviceId(uplink_out),
            target_lang: uplink_target,
            source: SourceLang::Auto,
        },
        downlink: LinkIntent {
            in_dev: DeviceId(downlink_in),
            out_dev: DeviceId(downlink_out),
            target_lang: downlink_target,
            source: SourceLang::Auto,
        },
    })
}

fn required_arg(args: &[String], key: &str) -> anyhow::Result<String> {
    arg_value(args, key).ok_or_else(|| anyhow::anyhow!("缺少参数 {key}"))
}

fn arg_value(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == key)
        .and_then(|index| args.get(index + 1).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::control::TranslateMode;

    #[test]
    fn parse_mode_defaults_and_variants() {
        assert_eq!(
            parse_mode(&["--mode".into(), "uplink-only".into()]),
            TranslateMode::UplinkOnly
        );
        assert_eq!(
            parse_mode(&["--mode".into(), "downlink-only".into()]),
            TranslateMode::DownlinkOnly
        );
        assert_eq!(parse_mode(&[]), TranslateMode::Bidirectional);
    }

    #[test]
    fn build_spec_maps_args_to_intents() {
        let args: Vec<String> = "--mode bidirectional --uplink-in PhysMic --uplink-out VirtMic --uplink-target en --downlink-in VirtSpk --downlink-out PhysHeadset --downlink-target zh"
            .split(' ')
            .map(String::from)
            .collect();
        let spec = build_spec(&args).unwrap();
        assert_eq!(spec.uplink.in_dev.0, "PhysMic");
        assert_eq!(spec.downlink.out_dev.0, "PhysHeadset");
        assert_eq!(spec.downlink.target_lang, "zh");
    }
}
