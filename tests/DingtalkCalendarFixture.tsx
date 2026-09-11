import { useState } from 'react';
import { createRoot } from 'react-dom/client';
import Calendar from '../src/features/Calendar';
import DingCalendarSettings from '../src/features/DingCalendarSettings';
import '../src/styles.css';
import '../src/glass.css';
import '../src/theme.css';
function Fixture(){
 const [settings,setSettings]=useState(false);
 return <main style={{padding:24}}><button type="button" onClick={()=>setSettings(value=>!value)}>{settings?'查看日历':'查看设置'}</button>{settings?<DingCalendarSettings/>:<Calendar tasks={[]} onEdit={()=>{throw Error('钉钉日程不可编辑为任务')}} onCreate={()=>{}} onChange={async()=>{throw Error('钉钉日程不可拖动')}}/>}</main>;
}
createRoot(document.getElementById('root')!).render(<Fixture/>);
