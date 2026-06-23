# M2 验收记录

> 需人工/QA 联调，未自动验证。以下项目需要真实 `GEMINI_API_KEY`、BlackHole/VB-CABLE、Zoom/Teams 或等价会议软件环境。

- 日期 / 平台（macOS / Windows）/ 虚拟设备（BlackHole / VB-CABLE）：
- 模式：bidirectional / uplink-only / downlink-only 各自结果：
- 双向 30 分钟：崩溃次数 / 明显循环回声 / 单链路断线另一条是否继续：
- 防循环：物理隔离拒绝启动；回环检测触发暂停的耗时与「配置修复」提示：
- VAD：静默期 saved_ratio、说话恢复及时性、阈值标定（rms_open/close/hangover）：
- 防双声：物理耳机是否只听译文：
- 热插拔：拔出物理麦 DeviceLost→Error 子态、插回刷新：
- 与 PRD 14.1 / 设计 §7 M2 验收的差距：

## hotfix 复测：延迟 / 回环

> 需人工/QA 联调，未自动验证真实音频效果。建议启动时设置：
>
> `RUST_LOG=engine=info,gemini_live=warn`

- 下行拖尾：停止远端说话后，物理耳机译文拖尾时长；观察是否明显低于修复前。
- 下行延迟代理日志：记录 `链路低频诊断` 中 `latency_proxy_ms`、`downstream_dropped_samples`、`downstream_dropped_ms` 的典型值和峰值。
- 播放端跟不上场景：人为制造输出设备慢/忙时，确认日志中下行丢帧计数递增，且不会无限堆积后持续播放旧译文。
- 回环触发：制造物理/应用层回环，记录触发 `LoopSuspected` / `TranslationPaused { AcousticLoop }` 的耗时，以及日志证据 `lag_frames`、`xcorr`、`ratio_db`。
- 回环暂停期间：确认该方向停止继续上行发送或停止注入译文，不再重复播放同一句译文。
- 回环恢复：解除回环后，确认满足滞回后出现 `TranslationResumed`，链路状态回到 `Running`。
- 误暂停检查：正常双人对话、短促咳嗽/键盘噪声/会议软件提示音场景下，记录是否误触发 Paused。
