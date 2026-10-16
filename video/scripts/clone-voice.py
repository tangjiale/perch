#!/usr/bin/env python3
"""已授权的参考音频近似合成；模型预先下载后完全离线推理，非豆包官方音色。"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import time
import wave

os.environ['HF_HUB_OFFLINE'] = '1'
os.environ['HF_HUB_DISABLE_IMPLICIT_TOKEN'] = '1'
os.environ['HF_HUB_DISABLE_TELEMETRY'] = '1'
os.environ['TRANSFORMERS_OFFLINE'] = '1'

import mlx.core as mx
import numpy as np
from mlx_audio.tts.utils import load_model

parser = argparse.ArgumentParser()
parser.add_argument('--model', required=True, help='已下载的本地模型目录；不会联网下载')
parser.add_argument('--reference-dir', default=str(Path.home() / '.codex/skills/wenrou-taozi-voice/assets'))
parser.add_argument('--text', default='打开栖点，我们先从左侧的我的任务开始。点击新建任务，填入今天要完成的事项。')
parser.add_argument('--output', default=str(Path(__file__).resolve().parents[1] / 'public/audio/taozi-sample.wav'))
args = parser.parse_args()
reference = Path(args.reference_dir)
model_path = Path(args.model).resolve()
if not (model_path / 'model.safetensors').is_file():
    raise RuntimeError('请先下载公共模型到本地目录，本脚本禁止联网下载')
mx.random.seed(42)
started = time.monotonic()
model = load_model(str(model_path))
chunks = []
sample_rate = None
for result in model.generate(text=args.text, ref_audio=str(reference / 'reference.wav'), ref_text=(reference / 'reference.txt').read_text().strip(), lang_code='Chinese', max_tokens=900, temperature=0.7, top_p=0.9, repetition_penalty=1.05, verbose=True):
    sample_rate = result.sample_rate
    chunks.append(np.array(result.audio).reshape(-1))
if not chunks:
    raise RuntimeError('模型没有生成音频')
audio = np.concatenate(chunks)
raw_duration = len(audio) / sample_rate
peak = float(np.max(np.abs(audio)))
if peak > 0.95:
    audio *= 0.95 / peak
# 保留模型完整输出，只追加静音，不裁切尾音或强行截长。
tail = audio[-int(sample_rate * .25):]
tail_rms = float(np.sqrt(np.mean(tail ** 2)))
audio = np.concatenate([audio, np.zeros(round(sample_rate * .4), dtype=np.float32)])
output = Path(args.output)
output.parent.mkdir(parents=True, exist_ok=True)
with wave.open(str(output), 'wb') as wav:
    wav.setnchannels(1)
    wav.setsampwidth(2)
    wav.setframerate(sample_rate)
    wav.writeframes((np.clip(audio, -1, 1) * 32767).astype('<i2').tobytes())
report = {'kind': '参考音频近似合成，非豆包官方原版', 'model': 'mlx-community/Qwen3-TTS-12Hz-0.6B-Base-4bit', 'localModelPath': str(model_path), 'referenceSha256': hashlib.sha256((reference / 'reference.wav').read_bytes()).hexdigest(), 'text': args.text, 'output': str(output), 'duration': len(audio)/sample_rate, 'rawDuration': raw_duration, 'sampleRate': sample_rate, 'peak': float(np.max(np.abs(audio))), 'tailRmsLast250ms': tail_rms, 'appendedSilence': .4, 'processingSeconds': time.monotonic()-started, 'offline': True, 'networkPolicy': 'HF_HUB_OFFLINE=1; TRANSFORMERS_OFFLINE=1; local model path only; reference read from local file'}
qa_dir = Path(__file__).resolve().parents[1] / 'out/qa'
qa_dir.mkdir(parents=True, exist_ok=True)
(qa_dir / f'{output.stem}.json').write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')
print(json.dumps(report, ensure_ascii=False, indent=2))
