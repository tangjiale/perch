// 仅模拟聊天 IPC，不调用模型服务。
import { execFileSync } from "node:child_process";
import assert from "node:assert/strict";
const origin = process.argv[2] || "http://127.0.0.1:1421";
const run = (...args) =>
  execFileSync(
    "npx",
    ["--yes", "agent-browser", "--session", "chat-model-test", ...args],
    { encoding: "utf8", timeout: 60000 },
  );
const ev = (code) =>
  JSON.parse(JSON.parse(run("eval", `JSON.stringify(${code})`).trim()));
const wait = () => run("wait", "200");
const select = (label, text) => {
  run("click", `[aria-label="${label}"][role=combobox]`);
  ev(
    `(()=>{const option=[...document.querySelectorAll('[role=option]')].find(e=>e.textContent.trim()===${JSON.stringify(text)});if(!option)throw Error('missing option');option.dataset.testChosen='yes';return true})()`,
  );
  run("click", "[data-test-chosen=yes]");
  wait();
};
const model = () =>
  ev(`document.querySelector('[aria-label="选择聊天模型"]').textContent`);
const send = () =>
  ev(
    `(()=>{document.querySelector('.chat-composer').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));return true})()`,
  );
try {
  run("open", `${origin}/tests/chat-models.html?empty=1`);
  wait();
  run("click", '[aria-label="选择聊天模型"]');
  assert.equal(
    ev(`document.querySelectorAll('[role=option]').length`),
    2,
    "禁用模型/供应商/embedding不能选择",
  );
  run("press", "Escape");
  select("选择聊天模型", "文本模型 · 本地测试服务");
  assert.match(model(), /文本模型/);
  assert.equal(
    ev(`window.__calls.filter(c=>c.name==='chat_set_model').length`),
    0,
    "草稿覆盖不调用已有会话IPC",
  );
  run("fill", "textarea", "使用选择的模型");
  send();
  wait();
  assert.equal(ev(`window.__sentModel`), "chat");
  assert.equal(
    ev(`window.__chatStore.agents[0].modelId`),
    "vision",
    "不改Agent默认",
  );
  select("选择 Agent", "文本助手");
  ev(
    `(()=>{[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='新会话').click();return true})()`,
  );
  wait();
  assert.match(model(), /文本模型/);
  run("open", `${origin}/tests/chat-models.html`);
  wait();
  run("fill", "textarea", "模型切换失败不丢草稿");
  ev(`window.__modelFail=true`);
  select("选择聊天模型", "文本模型 · 本地测试服务");
  assert.match(model(), /Qwen/);
  assert.equal(
    ev(`document.querySelector('textarea').value`),
    "模型切换失败不丢草稿",
  );
  assert.match(ev(`window.lastNotice`), /切换模型失败/);
  ev(
    `(()=>{window.__modelFail=false;window.__modelPending=true;window.__staleChatRefresh=true;window.__refreshFail=true;return true})()`,
  );
  select("选择聊天模型", "文本模型 · 本地测试服务");
  assert.equal(
    ev(`document.querySelector('[aria-label="选择聊天模型"]').disabled`),
    true,
  );
  assert.equal(
    ev(`document.querySelector('.chat-composer button[type=submit]').disabled`),
    true,
  );
  const sentBefore = ev(
    `window.__calls.filter(c=>c.name==='chat_send').length`,
  );
  send();
  assert.equal(
    ev(`window.__calls.filter(c=>c.name==='chat_send').length`),
    sentBefore,
    "保存中不能发送",
  );
  ev(
    `(()=>{window.__resolveModel();window.__modelPending=false;return true})()`,
  );
  wait();
  assert.match(model(), /文本模型/, "刷新失败不能覆盖持久化返回模型");
  assert.match(ev(`window.lastNotice`), /会话刷新失败/);
  send();
  wait();
  assert.equal(ev(`window.__sentModel`), "chat");
  assert.equal(
    ev(
      `window.__calls.filter(c=>c.name==='save_conversation').at(-1).args.value.revision`,
    ),
    2,
    "刷新失败后仍用持久化revision发送",
  );
  ev(`window.__refreshFail=false`);
  select("选择聊天模型", "Qwen3.5-VL-4B · 本地测试服务");
  assert.equal(
    ev(
      `window.__calls.filter(c=>c.name==='chat_set_model').at(-1).args.revision`,
    ),
    3,
    "后续切换使用最新revision",
  );
  select("选择聊天模型", "文本模型 · 本地测试服务");
  run("fill", "textarea", "新snapshot消息");
  ev(`window.__sendPending=true`);
  send();
  wait();
  assert.equal(
    ev(`document.querySelector('[aria-label="选择聊天模型"]').disabled`),
    true,
    "生成中禁止切换",
  );
  ev(`(()=>{window.__resolveSend();window.__sendPending=false;return true})()`);
  wait();
  assert.equal(ev(`window.__sentModel`), "chat", "发送使用新snapshot");
  ev(`(()=>{window.__staleChatRefresh=false;return true})()`);
  ev(
    `(()=>{[...document.querySelectorAll('.conversation-row > button:first-child')].find(b=>b.textContent.includes('文本助手')).click();return true})()`,
  );
  wait();
  ev(
    `(()=>{[...document.querySelectorAll('.conversation-row > button:first-child')].find(b=>b.textContent.includes('视觉助手')).click();return true})()`,
  );
  wait();
  assert.match(model(), /文本模型/, "重开同一会话保留模型");
  select("选择聊天模型", "Qwen3.5-VL-4B · 本地测试服务");
  ev(
    `(()=>{const canvas=document.createElement('canvas');canvas.width=2;canvas.height=2;const messages=[{id:'history-image',conversationId:'vision',role:'user',content:'历史图片内容',status:'complete',images:[{name:'history.png',dataUrl:canvas.toDataURL('image/png')}]}];window.__chatStore.messages=messages;window.updateChatFixture(messages);return true})()`,
  );
  wait();
  select("选择聊天模型", "文本模型 · 本地测试服务");
  assert.match(model(), /文本模型/, "历史图片可切文本");
  assert.equal(
    ev(
      `document.querySelectorAll('.chat-messages .chat-image-gallery img').length || document.querySelectorAll('.message .chat-image-gallery img').length`,
    ),
    1,
    "切模型不移除历史图片",
  );
  select("选择聊天模型", "Qwen3.5-VL-4B · 本地测试服务");
  ev(
    `(async()=>{const canvas=document.createElement('canvas');canvas.width=2;canvas.height=2;const blob=await new Promise(resolve=>canvas.toBlob(resolve,'image/png'));const transfer=new DataTransfer();transfer.items.add(new File([blob],'mock.png',{type:'image/png'}));const input=document.querySelector('[aria-label="选择聊天图片"]');input.files=transfer.files;input.dispatchEvent(new Event('change',{bubbles:true}));return true})()`,
  );
  wait();
  select("选择聊天模型", "文本模型 · 本地测试服务");
  assert.match(model(), /Qwen/);
  assert.match(ev(`window.lastNotice`), /先移除图片/);
  assert.equal(
    ev(
      `document.querySelectorAll('.chat-composer .chat-image-gallery img').length`,
    ),
    1,
    "切文本不能丢图片",
  );
  run("set", "viewport", "1440", "1000");
  ev(`document.documentElement.dataset.theme='light'`);
  run("screenshot", "/tmp/perch-chat-model-light.png");
  run("set", "viewport", "1024", "768");
  ev(`document.documentElement.dataset.theme='dark'`);
  run("screenshot", "/tmp/perch-chat-model-dark.png");
  console.log("聊天模型切换 18 项模拟浏览器验收通过");
} finally {
  run("close");
}
