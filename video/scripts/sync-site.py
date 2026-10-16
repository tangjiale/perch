#!/usr/bin/env python3
"""从最终录屏与旁白清单生成官网字幕和章节，避免沿用旧时间轴。"""
import json,math,re
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
clips=json.loads((ROOT/'capture-manifest.json').read_text())
voices={v['id']:v for v in json.loads((ROOT/'voice-manifest.json').read_text())}
assert [c['id']for c in clips]==['overview','tasks','calendar','chat','tools','backup']
def stamp(t):
 n=round(t*1000);return f'{n//3600000:02}:{n//60000%60:02}:{n//1000%60:02}.{n%1000:03}'
def clock(t):return f'{int(t)//60:02}:{int(t)%60:02}'
frames=0;cues=[];chapters=[]
for clip in clips:
 start=frames/30;chapters.append((start,clip['title']))
 for step in clip['steps']:
  v=voices[step['id']];a=start+round((step['start']+.1)*30)/30;b=a+math.ceil(v['duration']*30)/30
  assert b<=start+round(clip['duration']*30)/30+.035,step['id']
  cues.append({'start':a,'end':b,'text':v['text']})
 frames+=round(clip['duration']*30)
vtt='WEBVTT\n\n'+'\n\n'.join(f'{i+1}\n{stamp(c["start"])} --> {stamp(c["end"])}\n{c["text"]}' for i,c in enumerate(cues))+'\n'
(ROOT.parent/'docs/assets/perch-intro.zh-CN.vtt').write_text(vtt)
page=ROOT.parent/'docs/index.html';s=page.read_text()
s=s.replace('观看介绍 · 80 秒','观看操作演示').replace('80 秒，认识栖点。','跟着操作，上手栖点。').replace('A FIRST LOOK · 01:20',f'PRODUCT WALKTHROUGH · {clock(frames/30)}')
s=s.replace('从今天的总览出发，看看任务、日程与 AI，<br class="film-desktop-break">如何在一个工作台里接上你的工作。','从点击菜单到完成操作，跟着镜头走一遍。<br class="film-desktop-break">创建任务、安排日程，让日常工作更顺手。')
s=s.replace('栖点使用与产品介绍','栖点软件操作演示').replace('栖点-使用与产品介绍.mp4','栖点-软件操作演示.mp4').replace('下载介绍视频','下载操作视频')
s=s.replace('当前应用界面 · 匿名示例数据<br><span>按下播放','真实软件操作 · 匿名演示数据<br><span>按下播放')
s=re.sub(r'<div class="film-chapters".*?</div>', '<div class="film-chapters" role="group" aria-label="视频章节">'+''.join(f'<button type="button" data-video-start="{t:.3f}"><span>{clock(t)}</span>{title}</button>' for t,title in chapters)+'</div>',s,count=1)
s=re.sub(r'v=20261016(?:-operation)?', 'v=20261016-operation', s)
s=re.sub(r'PRODUCT WALKTHROUGH · \d{2}:\d{2}', f'PRODUCT WALKTHROUGH · {clock(frames/30)}', s)
page.write_text(s)
print(json.dumps({'duration':frames/30,'frames':frames,'chapters':chapters,'subtitles':len(cues)},ensure_ascii=False,indent=2))
