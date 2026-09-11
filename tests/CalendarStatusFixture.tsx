import { createRoot } from 'react-dom/client';
import { DateTime } from 'luxon';
import Calendar from '../src/features/Calendar';
import type { Task } from '../src/lib/types';
import '../src/styles.css';
import '../src/glass.css';
import '../src/theme.css';
const day=DateTime.local().startOf('day');
const tasks: Task[] = (['done','closed','doing'] as const).map((status,index)=>({
 id:status,title:status==='done'?'已完成禅道执行':status==='closed'?'已关闭禅道执行':'进行中禅道执行',
 notes:'',source:'zentao',status,priority:'normal',sortOrder:index,
 schedule:{kind:'all_day',start:day.toISODate()!,end:day.plus({days:1}).toISODate()!,timezone:day.zoneName!}
}));
createRoot(document.getElementById('root')!).render(<main style={{padding:24}}><Calendar tasks={tasks} onEdit={()=>{}} onCreate={()=>{}} onChange={async()=>{}}/></main>);
