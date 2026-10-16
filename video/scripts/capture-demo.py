#!/usr/bin/env python3
"""通过 agent-browser 录制真实 React 演示界面；所有写入只进入隔离 fixture 内存。"""
import json,re,subprocess,time,sys,os
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
SESSION='perch-operation'
VOICE={x['id']:x for x in json.loads((ROOT/'voice-manifest.json').read_text())}
CMD=([os.environ['PERCH_AGENT_BROWSER_BIN']] if os.environ.get('PERCH_AGENT_BROWSER_BIN') else ['npx','--yes','agent-browser'])+['--session',SESSION]

def run(*args,j=False):
    p=subprocess.run(CMD+(['--json'] if j else [])+list(args),text=True,capture_output=True,timeout=40)
    if p.returncode:raise RuntimeError(p.stdout+p.stderr)
    if j:
        d=json.loads(p.stdout)
        if not d.get('success'):raise RuntimeError(str(d))
        return d['data']
    return p.stdout

def ref(role,name,partial=False):
    snapshot=run('snapshot','-i')
    for line in snapshot.splitlines():
        m=re.search(r'- '+role+r' "([^"]*)".*?ref=(e\d+)',line)
        if m and (m[1]==name or (partial and name in m[1])):return '@'+m[2]
    raise RuntimeError(f'找不到 {role} {name}\n'+snapshot)

def move(selector):
    b=run('get','box',selector,j=True)
    if b['y']<0 or b['y']+b['height']>890:
        run('scrollintoview',selector);time.sleep(.4);b=run('get','box',selector,j=True)
    run('mouse','move',str(round(b['x']+b['width']/2)),str(round(b['y']+b['height']/2)),'--duration','600','--steps','24')

def click(selector):
    move(selector);run('mouse','down');time.sleep(.12);run('mouse','up');time.sleep(.24)

def control(role,name):click(ref(role,name))
def type_text(text):
    for i in range(0,len(text),3):
        run('keyboard','inserttext',text[i:i+3]);time.sleep(.08)

def fill(selector,text):
    click(selector)
    if 'data-video-date' in selector:
        # 原生日期控件使用浏览器标准 setter + input/change，避免 CLI 当普通文本清空值。
        run('eval',f'(()=>{{const e=document.querySelector({json.dumps(selector)});Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(e,{json.dumps(text)});e.dispatchEvent(new Event("input",{{bubbles:true}}));e.dispatchEvent(new Event("change",{{bubbles:true}}));}})()')
        run('press','Escape')
    else:run('fill',selector,text)

def navigate(path):click(f'a[href="#{path}"]')

scene=sys.argv[1]
manifest_path=ROOT/'capture-manifest.json'
manifest=json.loads(manifest_path.read_text()) if manifest_path.exists() else []
manifest=[x for x in manifest if x.get('id')!=scene]
file=f'recordings/{scene}.mp4'
run('record','start',str(ROOT/'public'/file),'--fps','30','--cursor')
t0=time.monotonic();steps=[]
def step(key,fn=lambda:None,zoom=1,x=.5,y=.5,delay=.2,min_seconds=0):
    start=time.monotonic()-t0
    fn()
    target=max(VOICE[key]['duration']+.75,min_seconds)
    remaining=target-(time.monotonic()-t0-start)
    if remaining>0:time.sleep(remaining)
    steps.append(dict(id=key,start=round(start,3),end=round(time.monotonic()-t0,3),zoom=zoom,x=x,y=y,delay=delay))
    print(key,steps[-1],flush=True)

try:
    if scene=='overview':
        step('overview',lambda:run('mouse','move','85','121','--duration','700','--steps','30'))
        step('overview_detail',lambda:run('mouse','move','650','365','--duration','800','--steps','30'),zoom=1.22,x=.54,y=.43)
    elif scene=='tasks':
        step('tasks_menu',lambda:navigate('/tasks'))
        def new():
            control('button','新建任务');click(ref('textbox','任务名称'));type_text('整理本周工作计划')
        step('task_new',new,zoom=1.35,x=.50,y=.30,delay=1.6,min_seconds=5)
        def schedule():
            control('checkbox','全天安排')
            run('eval', 'document.querySelectorAll("input[type=date]").forEach((e,i)=>e.setAttribute("data-video-date",String(i)))')
            fill('input[data-video-date="0"]','2026-10-19')
            # 日期输入位于不同标签下，用当前 DOM 的第二项定位。
            fill('input[data-video-date="1"]','2026-10-20')
        step('task_schedule',schedule,zoom=1.5,x=.50,y=.43,min_seconds=5)
        def reminder():
            control('checkbox','到期提醒')
            control('combobox','提醒时间')
            control('option','提前 15 分钟')
        step('task_reminder',reminder,zoom=1.5,x=.5,y=.58,min_seconds=4.5)
        step('task_save',lambda:control('button','保存'),zoom=1,delay=.1)
        def status():
            control('button','整理本周工作计划');control('combobox','任务状态');control('option','正在做')
        step('task_status',status,zoom=1.4,x=.5,y=.32,delay=1,min_seconds=5)
        step('task_saved',lambda:control('button','保存'),zoom=1,delay=.1)
    elif scene=='calendar':
        step('calendar_menu',lambda:navigate('/calendar'))
        step('calendar_week',lambda:control('button','周'),zoom=1.18,x=.60,y=.46,delay=1.1)
        def sources():
            c=ref('checkbox','钉钉日历');click(c);time.sleep(.5);click(c)
        step('calendar_sources',sources,zoom=1.4,x=.35,y=.53)
    elif scene=='chat':
        step('chat_menu',lambda:navigate('/chat'))
        def knowledge():
            control('combobox','选择知识库');control('option','产品知识库')
        step('chat_knowledge',knowledge,zoom=1.45,x=.65,y=.79,delay=.4)
        def message():
            click(ref('textbox','消息'));type_text('帮我整理本周的工作计划。')
        step('chat_input',message,zoom=1.5,x=.63,y=.78,min_seconds=5)
        step('chat_send',lambda:move(ref('button','发送')),zoom=1.25,x=.69,y=.75)
    elif scene=='tools':
        step('tools_menu',lambda:navigate('/tools'))
        def exchange():
            click(ref('button','汇率转换',True));fill('#exchange-amount','1000')
        step('tools_exchange',exchange,zoom=1.4,x=.53,y=.46,delay=1.6,min_seconds=5)
    elif scene=='backup':
        def storage():
            navigate('/settings');control('tab','数据与备份')
        step('backup_menu',storage,zoom=1.15,x=.55,y=.28,delay=1.6)
        step('backup_local',lambda:move(ref('button','导出完整备份')),zoom=1.35,x=.55,y=.47)
        step('ending',lambda:navigate('/'),zoom=1)
    else:raise RuntimeError('未知场景')
except Exception:
    run('record','stop')
    raise
run('record','stop')
duration=float(subprocess.check_output(['ffprobe','-v','error','-show_entries','format=duration','-of','csv=p=0',str(ROOT/'public'/file)],text=True))
manifest.append(dict(id=scene,title={'overview':'工作总览','tasks':'创建与推进任务','calendar':'日历排期','chat':'AI 工作伙伴','tools':'日常工具','backup':'数据与备份'}[scene],file=file,duration=duration,steps=steps))
order=['overview','tasks','calendar','chat','tools','backup'];manifest.sort(key=lambda c:order.index(c['id']))
manifest_path.write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
print('RECORDED',scene,duration,flush=True)
