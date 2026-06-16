# TASK-QA-003 · 阶段 M2 双向链路 + 设备管理 + 防循环 + VAD（测试）

> QA Agent（Claude Sonnet 子代理）执行。依据 `TASK/delivery/DELIVERY-003.md` 与本文件验证 M2。规范见 `qa.md`。

## 验收门槛
无 P0/P1/P2 即通过（真实 API/真机联调项另列为"需人工验证"，不计入自动 Bug 门槛，但须如实记录状态）。

## 必跑命令（贴真实输出）
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
cargo run -p cli            # 应打印分类设备表（[default]/[virtual-mic]/[virtual-speaker]/[physical]）
```

## 用例清单

| # | crate | 用例 | 期望 |
|---|---|---|---|
| 1 | audio-core | watch 句柄转发 | `DeviceWatchHandle.try_recv` 初始为空；发送 `RawDeviceEvent::ListChanged` 后可收到 |
| 2 | device-manager | 分类 | VB-CABLE Output 方向→VirtualMic；BlackHole Input 方向→VirtualSpeaker；物理麦→Physical |
| 3 | device-manager | 快照 diff | `[A,B]→[B,C]` 排序后恰 `[Added{C},Removed{A}]`，B 无事件 |
| 4 | device-manager | 默认变更 | 集合不变、默认从 A 迁 B → 恰 `[DefaultChanged{B}]`，无 Added/Removed |
| 5 | device-manager | DeviceLost | 在用设备缺失→`DeviceLost`；仍在→空 Vec（区别于普通 Removed） |
| 6 | device-manager | 不可变刷新 | mock 改变后 `refresh()` 新快照 `old != new`；原始事件经 `diff_snapshots`→语义 Added 且 `classify==VirtualSpeaker`，全程无 cpal |
| 7 | diagnostics | 能量摘要 | 全零→rms=0/peak=0；满幅→peak=i16::MAX、rms 接近满刻度；空切片 n=0 不 panic |
| 8 | diagnostics | 隔离·源汇重叠 | 同设备作源与汇→`Err(SourceSinkOverlap/OutputIsVirtualCaptureSource)`；合法双链路→Ok |
| 9 | diagnostics | 隔离·输出回流虚拟采集 | 汇是另一链路的虚拟采集源→`Err(OutputIsVirtualCaptureSource)`；物理耳机作汇→Ok |
| 10 | diagnostics | 回环命中 | captured=injected 右移 12 帧、-3dB→`suspected==true`、lag∈[11,13]、ratio_db≈-3 |
| 11 | diagnostics | 回环不误判 | 远低于注入(ratio<-6dB) 或 高能不相关(xcorr<0.6)→`suspected==false` |
| 12 | diagnostics | 滞回 Pause/Resume | 连 hold 帧疑似→`Pause` 进 Paused；单帧 Clear 不恢复；连 release 帧 Clear→`Resume`；抖动不翻转 |
| 13 | audio-dsp | VAD 静音 | `observe(&[0;480])==Drop`；100 帧全零 `frames_sent==0`、`saved_ratio==1.0` |
| 14 | audio-dsp | VAD attack | 大音量连 attack 帧后转 `Send`、`is_speaking()==true` |
| 15 | audio-dsp | VAD 幅度滞回 | 边界 RMS(close<rms<open)：静音态→Drop；已 speaking→Send（不抖动） |
| 16 | audio-dsp | VAD hangover | 语音后切静音：连 hangover 帧仍 Send（补尾），之后才 Drop 且 `is_speaking()==false` |
| 17 | audio-dsp | RMS 纯函数 | 全零→0；直流满幅→幅度（误差≤1）；空帧→0 不 panic；同输入多次一致 |
| 18 | audio-dsp | 高 ZCR 低能判噪声 | 高翻转低幅帧：`zcr` 高、`classify_frame(..,false)==false`、`observe==Drop` |
| 19 | audio-cpal | watch 句柄不崩 | `watch_devices()` 返回句柄、`try_recv` 不阻塞不 panic（CI 无声卡亦可） |
| 20 | engine | Paused 投影 | `worst_state(Paused, Reconnecting)==Paused`；`worst_state(Error, Paused)==Error` |
| 21 | engine | 三态装配 | `active_links` 三态正确；UplinkOnly→`downlink=None`；Bidirectional→两链路、downlink `source==Auto`、四设备互异 |
| 22 | engine | 隔离拒绝 | 上行注入==下行采集→`Err(VirtualLoopback/SourceSinkOverlap)`；合法→Ok |
| 23 | engine | 缺设备干净失败 | spec 引用不存在设备→`Err(DeviceNotFound)`，不 panic |
| 24 | engine | 编排器投影 | `top_state` 复用 `worst_state`；relay 发出独立 `UplinkState(Running)` 与 `DownlinkState(Reconnecting{2})` |
| 25 | cli | 参数解析 | `parse_mode` 三态 + 默认 bidirectional；`build_spec` 正确映射上下行 in/out/target |
| 26 | cli | 分类设备表 | 无参运行打印 `[default]/[virtual-mic]/[virtual-speaker]/[physical]` 标签 |

## 架构纪律核查
- [ ] `engine`/`device-manager`/`diagnostics`/`cli` 源码无 `#[cfg(target_os)]`（`grep -rn 'cfg(target_os' crates/{engine,device-manager,diagnostics,cli}/src` 为空）；平台差异只在 `audio-cpal`。
- [ ] `device-manager`/`diagnostics` 的 `Cargo.toml` 不依赖 `cpal`/`audio-cpal`（核对依赖段）。
- [ ] 数据面纪律：`frame_energy`/VAD 按 `&[i16]` 借用、无堆分配（核对函数体无 `Vec::new`/`vec!`/`.to_vec()` 等于热路径）；音频回调无 await/锁/分配。
- [ ] 第二条下行链路 = 独立 Session/独立 task/独立重连/独立过期帧丢弃；`worst_state` 复用未重写。
- [ ] downlink 走"听者定目标"：`source==Auto`、target 锁听者语言；远端音频不建立"远端→物理输出"直连边（防双声结构性保证）。
- [ ] `cargo build --workspace` 闭合 Task 14 trait 破坏性缺口（audio-cpal 已实现 watch_devices）。
- [ ] `GEMINI_API_KEY` 走环境变量，无硬编码密钥；交付物无 `unimplemented!`/`TODO`/`FIXME`。

## 待人工验证项（Task 26，记录状态即可）
- [ ] 真机中英双向 30 分钟：本地中文→对方英文、对方英文→本地中文；不崩、无明显循环回声（设计 §7 M2 / PRD 14.1）。
- [ ] 两链路独立性：一条临时断网另一条仍 Running 自动重连。
- [ ] 三态省成本：仅上行/仅下行时对应方向 Gemini 连接未建立（可观测）。
- [ ] 防循环第二道防线：真实声学环路数秒内 `Paused` + CLI"配置修复"提示，不依赖 AEC；30 分钟正常对话误暂停 0 次。
- [ ] 防双声主观：物理耳机只听译文、不出现远端英文原声。
- [ ] VAD 真机阈值标定：静默期 saved_ratio 上升、句首/句尾不截断、噪声不误触发。
- [ ] 热插拔：拔出物理麦数秒内 DeviceLost→Error 子态不崩溃，插回刷新；空列表保护不误报全量 Removed。
- [ ] 虚拟设备真实命名（BlackHole/VB-CABLE 两平台）被 `is_virtual_device_name` 命中；漏判时结构性隔离兜底。
- [ ] `docs/superpowers/notes/M2-validation.md` 是否就位（可为待填模板）。

## 报告
生成 `TASK/reports/QA-REPORT-003-r{round}.md`（见 `qa.md §5`）。含 P0/P1/P2 则打回 Dev。重点核对：纯函数测试是否**真实覆盖**断言（非默认相信）；架构纪律 grep 结果；回环/VAD 滞回边界；数据面零分配。