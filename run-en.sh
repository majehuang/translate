#!/usr/bin/env bash
# 中文 → 英文实时翻译（对麦克风说中文，耳机出英文）。
# 用法：
#   GEMINI_API_KEY=<你的key> ./run-en.sh
# 可选环境变量覆盖设备：
#   IN_DEVICE="外置麦克风" OUT_DEVICE="外置耳机" GEMINI_API_KEY=<key> ./run-en.sh
set -euo pipefail

cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

if [[ -z "${GEMINI_API_KEY:-}" ]]; then
  echo "错误：未设置 GEMINI_API_KEY 环境变量。" >&2
  echo "用法：GEMINI_API_KEY=<你的key> ./run-en.sh" >&2
  exit 1
fi

IN_DEVICE="${IN_DEVICE:-外置麦克风}"
OUT_DEVICE="${OUT_DEVICE:-外置耳机}"

echo "中→英  输入：${IN_DEVICE}  输出：${OUT_DEVICE}  目标语言：en"
echo "提示：看到 'Gemini 已连接' 后对麦克风说中文；Ctrl+C 停止。"

exec cargo run --release -p cli -- \
  --in-device "${IN_DEVICE}" \
  --out-device "${OUT_DEVICE}" \
  --target en
