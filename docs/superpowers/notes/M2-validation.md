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
