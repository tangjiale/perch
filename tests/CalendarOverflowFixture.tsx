import { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { DateTime } from 'luxon';
import Calendar from '../src/features/Calendar';
import type { Task } from '../src/lib/types';
import '../src/styles.css';
import '../src/glass.css';
import '../src/theme.css';

const day = DateTime.local().startOf('month').plus({ days: 11 });
const targetDate = day.toISODate()!;
const task = (id: string, status: Task['status'], start: number, duration: number): Task => ({
  id, title: id, notes: '', source: 'local', status, priority: 'normal', sortOrder: 0,
  schedule: { kind: 'all_day', start: day.plus({ days: start }).toISODate()!, end: day.plus({ days: start + duration }).toISODate()!, timezone: day.zoneName! },
});
const crowded = [
  task('已完成长跨天', 'done', -4, 10), task('已关闭长跨天', 'closed', -3, 9),
  task('未完成早开始', 'todo', -1, 4), task('进行中当天', 'doing', 0, 2), task('未完成当天', 'todo', 0, 1),
];
function Fixture() {
  const [tasks, setTasks] = useState(crowded);
  return <main style={{ padding: 24 }}>
    <div data-target-date={targetDate} style={{ marginBottom: 12 }}>
      <button data-scenario="crowded" onClick={() => setTasks(crowded)}>拥挤场景</button>
      <button data-scenario="few" onClick={() => setTasks([task('未完成当天', 'todo', 0, 1), task('已完成长跨天', 'done', -4, 10), task('已关闭长跨天', 'closed', -3, 9)])}>少量场景</button>
      <button data-scenario="complete" onClick={() => setTasks(crowded.map(item => item.id === '未完成早开始' ? { ...item, status: 'done' } : item))}>完成早开始</button>
    </div>
    <Calendar tasks={tasks} onEdit={() => {}} onCreate={() => {}} onChange={async () => {}} />
  </main>;
}
createRoot(document.getElementById('root')!).render(<Fixture />);
