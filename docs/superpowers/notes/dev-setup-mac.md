# 在 macOS 上搭建开发环境

> 把开发主力从 Windows 切到 Mac 的完整步骤。代码已在 GitHub（SSH），
> 本质是"克隆 + 配环境 + 继续执行计划"。对本项目 Mac 是更顺的主力机
> （CoreAudio / BlackHole 生态更成熟）。

仓库：`git@github.com:majehuang/translate.git`
开发分支：`feat/m0-m1-foundation`

---

## A. 配 GitHub SSH（每台机器一把独立密钥最干净）

```bash
# 1) 生成 Mac 专用密钥
ssh-keygen -t ed25519 -C "asloate4@maje.ac.cn" -f ~/.ssh/github_ed25519 -N ""

# 2) 打印公钥，复制整行
cat ~/.ssh/github_ed25519.pub

# 3) 让 github.com 用这把密钥
cat >> ~/.ssh/config <<'EOF'

Host github.com
    HostName github.com
    IdentityFile ~/.ssh/github_ed25519
    IdentitiesOnly yes
EOF
```

把第 2 步的公钥贴到 https://github.com/settings/ssh/new ，然后验证：

```bash
ssh -T git@github.com    # 出现 "Hi majehuang!" 即成功
```

---

## B. 克隆仓库

```bash
git clone git@github.com:majehuang/translate.git
cd translate
git checkout feat/m0-m1-foundation
```

设计 spec 与实现计划都在 `docs/superpowers/` 下，随仓库一起克隆。

---

## C. 装 Rust 工具链（macOS）

```bash
# 1) C 链接器 / clang（CPAL 等需要；可能已装）
xcode-select --install        # 弹窗点安装；已装会提示 already installed

# 2) Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # 一路默认回车
source "$HOME/.cargo/env"
cargo --version               # 验证
```

- macOS 上 CPAL 直接用系统 CoreAudio，**M0/M1 无需额外音频驱动**。
- BlackHole（虚拟设备）等到 **M2** 才用，到时再装：`brew install blackhole-2ch`。

---

## D. 继续执行实现计划

在 Mac 上 `cd translate` 后**重新启动 Claude Code**，用这句话接上：

> 执行计划 `docs/superpowers/plans/2026-06-13-m0-foundation-m1-gemini-link.md`，
> 用 subagent 驱动从 Task 1 开始。当前分支 feat/m0-m1-foundation。

新会话会读取计划文件，从 Task 1（建 Cargo workspace + 双平台 CI）逐任务推进。
Task 13 的最终联调需要真实的 `GEMINI_API_KEY` 环境变量。

---

## 备注

- Windows 那台无需收尾：分支均已推送；唯一未提交的是机器本地的
  `.claude/settings.local.json`（本机设置，不入仓库）。
- 若 Windows 仍作为第二联调平台：保持双平台并行，计划与 CI 已覆盖，
  以后 `git pull` 即可。若暂时只做 Mac，可把计划中"双平台并行"约束
  放宽为"Mac 优先、Windows 暂缓"。
- GitHub 仓库默认分支若为 `main` 而本地主干为 `master`，可在仓库
  Settings 调整默认分支，或将本地 `master` 改名为 `main` 重推。
