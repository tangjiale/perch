#!/usr/bin/env python3
"""同一模型进程逐句离线合成已授权的参考近似声音，不修改输入文稿。"""
import os
os.environ.update(HF_HUB_OFFLINE='1', TRANSFORMERS_OFFLINE='1', HF_HUB_DISABLE_IMPLICIT_TOKEN='1', HF_HUB_DISABLE_TELEMETRY='1')
import argparse
import json
from pathlib import Path
import time
import wave
import mlx.core as mx
import numpy as np
from mlx_audio.tts.utils import load_model
ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--model', required=True, help='已下载的本地 Qwen3-TTS Base 模型目录')
parser.add_argument('--reference-dir', default=str(Path.home() / '.codex/skills/wenrou-taozi-voice/assets'))
parser.add_argument('--asr-model', help='可选的本地 Whisper 模型目录；省略则跳过 ASR')
args = parser.parse_args()
REF = Path(args.reference_dir).expanduser().resolve()
MODEL = Path(args.model).expanduser().resolve()
if not (MODEL / 'model.safetensors').is_file():
    parser.error('--model 必须指向已有本地模型目录，本脚本禁止联网下载')
if not (REF / 'reference.wav').is_file() or not (REF / 'reference.txt').is_file():
    parser.error('--reference-dir 必须包含 reference.wav 和 reference.txt')
asr_model = Path(args.asr_model).expanduser().resolve() if args.asr_model else None
if asr_model is not None and not (asr_model / 'weights.npz').is_file() and not (asr_model / 'model.safetensors').is_file():
    parser.error('--asr-model 必须指向已有本地 Whisper 模型目录')
OUTPUT = ROOT/'public/audio/taozi'
OUTPUT.mkdir(parents=True,exist_ok=True)
rows=json.loads((ROOT/'narration-script.json').read_text())
model=load_model(str(MODEL))
manifest=[]
for index,row in enumerate(rows):
    mx.random.seed(42 if row['id']=='chat_input' else 42+index)
    started=time.monotonic()
    result=list(model.generate(text=row['text'],ref_audio=str(REF/'reference.wav'),ref_text=(REF/'reference.txt').read_text().strip(),lang_code='Chinese',max_tokens=700,temperature=.7,top_p=.9,repetition_penalty=1.05,verbose=False))
    sr=result[0].sample_rate
    audio=np.concatenate([np.array(chunk.audio).reshape(-1) for chunk in result])
    peak=float(np.max(np.abs(audio)))
    ceiling=.95 if row['id']=='chat_input' else .94
    if peak>ceiling:audio*=ceiling/peak
    raw_duration=len(audio)/sr
    padding=.4 if row['id']=='chat_input' else .35
    audio=np.concatenate([audio,np.zeros(round(sr*padding),dtype=np.float32)])
    file=OUTPUT/f"{row['id']}.wav"
    with wave.open(str(file),'wb') as w:
        w.setnchannels(1);w.setsampwidth(2);w.setframerate(sr);w.writeframes((audio*32767).astype('<i2').tobytes())
    manifest.append({'id':row['id'],'file':f"audio/taozi/{row['id']}.wav",'duration':round(len(audio)/sr,6),'text':row['text']})
    (ROOT/'voice-manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps({'index':index+1,**manifest[-1],'rawDuration':round(raw_duration,3),'processingSeconds':round(time.monotonic()-started,2)},ensure_ascii=False),flush=True)
print('ALL_GENERATED',flush=True)
# 已有本地 Whisper 模型复核，不下载 ASR 权重、不上传音频。
del model
mx.clear_cache()
if asr_model is not None:
    import mlx_whisper
    qa_dir = ROOT / 'out/qa'
    qa_dir.mkdir(parents=True, exist_ok=True)
    checks=[]
    for row in manifest:
        r=mlx_whisper.transcribe(str(ROOT/'public'/row['file']),path_or_hf_repo=str(asr_model),language='zh')
        check={'id':row['id'],'expected':row['text'],'recognized':r['text']}
        checks.append(check)
        (qa_dir/'taozi-asr-checks.json').write_text(json.dumps(checks,ensure_ascii=False,indent=2)+'\n')
        print('ASR '+json.dumps(check,ensure_ascii=False),flush=True)
else:
    print('ASR_SKIPPED: 未指定本地 --asr-model', flush=True)
