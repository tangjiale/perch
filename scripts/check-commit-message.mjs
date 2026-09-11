import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
const args = process.argv.slice(2);
try {
  const messages = args[0] === '--range'
    ? execFileSync('git', ['log', '--no-merges', '--format=%s', args[1]], { encoding: 'utf8' }).trim().split('\n').filter(Boolean)
    : [readFileSync(args[0], 'utf8').split(/\r?\n/).find(line => line.trim() && !line.startsWith('#')) || ''];
  if (messages.some(subject => !/\p{Script=Han}/u.test(subject))) throw Error('提交主题必须包含中文说明，例如：修复：日历任务显示异常');
  console.log('提交说明中文检查通过');
} catch (error) { console.error(error.message); process.exitCode = 1; }
