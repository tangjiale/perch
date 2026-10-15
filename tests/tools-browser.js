// 仅模拟 IPC，不操作真实 NATAPP 或凭据。
import { execFileSync } from "node:child_process";
import assert from "node:assert/strict";
const origin = process.argv[2] || "http://127.0.0.1:1421";
const run = (...args) =>
  execFileSync(
    "npx",
    ["--yes", "agent-browser", "--session", "tools-test", ...args],
    { encoding: "utf8", timeout: 60000 },
  );
const ev = (code) =>
  JSON.parse(JSON.parse(run("eval", `JSON.stringify(${code})`).trim()));
const wait = () => run("wait", "250");
const click = (text) =>
  ev(
    `(()=>{const b=[...document.querySelectorAll('button')].find(x=>x.textContent.trim()===${JSON.stringify(text)});if(!b)throw Error('button missing');b.click();return true})()`,
  );
try {
  if (process.argv.includes("--addresses")) {
    run("open", `${origin}/tests/tools.html?page=/tools/natapp`);
    wait();
    ev(
      `(() => { window.__state.installed=true;window.__state.tunnels=[{id:'online',name:'开发服务',revision:1,hasAuthToken:true,status:'running'},{id:'offline',name:'待启动服务',revision:1,hasAuthToken:true,status:'stopped'}];[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='刷新').click();const invoke=window.__TAURI_INTERNALS__.invoke;window.__TAURI_INTERNALS__.invoke=async(name,args)=>{const result=await invoke(name,args);if(name==='natapp_stop'){const tunnel=window.__state.tunnels.find(t=>t.id===args.id);tunnel.forwardingUrl=null;tunnel.localAddress=null;}return result;};return true; })()`,
    );
    wait();
    assert.equal(
      ev(`document.querySelectorAll('.natapp-addresses').length`),
      0,
      "未收到地址时不显示空字段",
    );
    ev(
      `(() => {const tunnel=window.__state.tunnels[0];tunnel.forwardingUrl='http://example-test.natappfree.cc';tunnel.localAddress='127.0.0.1:8080';return true;})()`,
    );
    run("wait", "1700");
    assert.match(
      ev(`document.querySelector('.natapp-addresses').textContent`),
      /访问地址http:\/\/example-test.natappfree.cc本地映射127.0.0.1:8080/,
    );
    ev(
      `(() => {window.__state.tunnels[0].forwardingUrl='https://'+ 'long-address'.repeat(15) +'.example.test';return true;})()`,
    );
    run("wait", "1700");
    run("set", "viewport", "1100", "800");
    ev(`document.documentElement.dataset.theme='light'`);
    assert.equal(
      ev(
        `document.querySelector('.natapp-row').scrollWidth > document.querySelector('.natapp-row').clientWidth`,
      ),
      false,
      "长地址不撑宽",
    );
    const light = ev(
      `({start:getComputedStyle(document.querySelector('.natapp-start-icon')).color,stop:getComputedStyle(document.querySelector('.natapp-stop-icon')).color})`,
    );
    assert.notEqual(light.start, light.stop);
    assert.equal(light.start, "rgb(40, 150, 101)");
    assert.equal(light.stop, "rgb(199, 53, 53)");
    run("screenshot", "/tmp/perch-natapp-addresses-light.png");
    run("set", "viewport", "760", "800");
    ev(`document.documentElement.dataset.theme='dark'`);
    const dark = ev(
      `({start:getComputedStyle(document.querySelector('.natapp-start-icon')).color,stop:getComputedStyle(document.querySelector('.natapp-stop-icon')).color})`,
    );
    assert.equal(dark.start, "rgb(139, 212, 173)");
    assert.equal(dark.stop, "rgb(242, 163, 163)");
    assert.equal(
      ev(
        `document.querySelector('.natapp-row').scrollWidth > document.querySelector('.natapp-row').clientWidth`,
      ),
      false,
    );
    run("screenshot", "/tmp/perch-natapp-addresses-dark.png");
    click("停止");
    wait();
    assert.equal(
      ev(`document.querySelectorAll('.natapp-addresses').length`),
      0,
      "停止后清空映射地址",
    );
    console.log("NATAPP 映射地址与图标颜色 8 项针对性验收通过");
  } else if (process.argv.includes("--install-failure")) {
    run("open", `${origin}/tests/tools.html?page=/tools/natapp`);
    wait();
    ev(
      `(() => { window.__state.tunnels=[{id:'failure-test',name:'模拟隧道',revision:1,hasAuthToken:true,status:'stopped'}];[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='刷新').click();return true; })()`,
    );
    wait();
    ev(
      `(() => { const original=window.__TAURI_INTERNALS__.invoke;let failing=true;window.__TAURI_INTERNALS__.invoke=async(name,args)=>{if(name==='natapp_install'&&failing){window.__state.installing=true;return new Promise((resolve,reject)=>window.__rejectInstall=()=>{failing=false;window.__state.installing=false;reject('官方安装信息格式已变化');});}if(name==='natapp_state'&&window.__state.installing){const old=structuredClone(window.__state);return new Promise(resolve=>window.__resolveOldInstalling=()=>resolve(old));}return original(name,args);};return true;})()`,
    );
    click("下载并安装");
    run("wait", "1700");
    assert.equal(
      ev(`!!document.querySelector('.natapp-download')`),
      true,
      "安装执行中显示进度",
    );
    ev(`(() => { window.__rejectInstall(); return true; })()`);
    wait();
    assert.equal(
      ev(`!!document.querySelector('.natapp-download')`),
      false,
      "安装失败移除进度",
    );
    assert.equal(
      ev(
        `[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='下载并安装').disabled`,
      ),
      false,
      "失败后可重试安装",
    );
    ev(`(() => { window.__resolveOldInstalling(); return true; })()`);
    wait();
    assert.equal(
      ev(`!!document.querySelector('.natapp-download')`),
      false,
      "迟到installing快照不能恢复进度",
    );
    run("wait", "1700");
    assert.match(
      ev(`document.querySelector('[role=alert]').textContent`),
      /官方安装信息格式已变化/,
      "轮询保留错误提示",
    );
    click("下载并安装");
    wait();
    assert.match(
      ev(`document.body.textContent`),
      /客户端已安装/,
      "失败后可重新安装成功",
    );
    console.log("NATAPP 安装失败恢复 6 项针对性验收通过");
    process.exitCode = 0;
  } else {
    run("open", `${origin}/tests/tools.html`);
    wait();
    assert.equal(ev(`document.querySelectorAll('.tool-card').length`), 2);
    ev(`document.querySelector('.tool-card').click()`);
    wait();
    run("fill", "textarea", "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
    wait();
    assert.match(
      ev(`document.querySelector('output').textContent`),
      /^\d{3} \d{3}$/,
    );
    assert.ok(
      ev(
        `Number(document.querySelector('[role=progressbar]').getAttribute('aria-valuenow'))`,
      ) <= 100,
    );
    ev(
      `(()=>{Date.now=()=>59000;Object.defineProperty(navigator,'clipboard',{value:{writeText:async text=>{window.__copied=text}},configurable:true});window.dispatchEvent(new Event('focus'));return true})()`,
    );
    wait();
    assert.equal(ev(`document.querySelector('output').textContent`), "287 082");
    click("复制验证码");
    wait();
    assert.equal(ev(`window.__copied`), "287082");
    ev(
      `(()=>{Date.now=()=>60000;window.dispatchEvent(new Event('focus'));return true})()`,
    );
    wait();
    assert.notEqual(
      ev(`document.querySelector('output').textContent`),
      "287 082",
    );
    click("清空");
    wait();
    assert.equal(ev(`document.querySelector('textarea').value`), "");
    assert.match(ev(`document.querySelector('output').textContent`), /—/);
    run("fill", "textarea", "otpauth://hotp/account?secret=MY");
    wait();
    assert.match(
      ev(`document.querySelector('[role=alert]').textContent`),
      /TOTP/,
    );
    run("fill", "textarea", "bad123");
    wait();
    assert.match(
      ev(`document.querySelector('[role=alert]').textContent`),
      /Base32/,
    );
    run("open", `${origin}/tests/tools.html?page=/tools/natapp`);
    wait();
    assert.equal(ev(`document.querySelector('button.primary').disabled`), true);
    click("添加隧道");
    wait();
    run("fill", "dialog input:not([type=password])", "开发隧道");
    run("fill", "dialog input[type=password]", "0123456789abcdef");
    ev(
      `(() => { const form=document.querySelector('dialog form'); form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true})); form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true})); return true; })()`,
    );
    wait();
    assert.equal(
      ev(`window.__calls.filter(c=>c.name==='natapp_save').length`),
      1,
      "同帧重复保存只发一个写入",
    );
    assert.equal(ev(`document.querySelectorAll('.natapp-row').length`), 1);
    assert.equal(
      ev(
        `[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='启动').disabled`,
      ),
      true,
    );
    click("下载并安装");
    wait();
    click("启动");
    wait();
    assert.match(
      ev(`document.querySelector('.natapp-status').textContent`),
      /运行中/,
    );
    assert.equal(
      ev(`document.querySelector('button[title="编辑隧道"]').disabled`),
      true,
    );
    click("日志");
    wait();
    assert.match(
      ev(`document.querySelector('.natapp-logs').textContent`),
      /Tunnel started/,
    );
    run("click", 'dialog button[title="关闭"]');
    click("停止");
    wait();
    run("click", 'button[title="编辑隧道"]');
    wait();
    assert.equal(
      ev(`document.querySelector('dialog input[type=password]').value`),
      "",
    );
    run("fill", "dialog input:not([type=password])", "修改名称");
    ev(`window.__fail=true`);
    click("保存");
    wait();
    assert.equal(
      ev(`document.querySelector('dialog input:not([type=password])').value`),
      "修改名称",
    );
    assert.match(
      ev(`document.querySelector('dialog [role=alert]').textContent`),
      /模拟操作失败/,
    );
    ev(`window.__fail=false`);
    run("wait", "1700");
    assert.match(
      ev(`document.querySelector('dialog [role=alert]').textContent`),
      /模拟操作失败/,
      "状态轮询不能清除操作错误",
    );
    assert.equal(
      ev(`document.querySelector('dialog input:not([type=password])').value`),
      "修改名称",
    );
    click("保存");
    wait();
    assert.equal(
      ev(
        `Object.hasOwn(window.__calls.findLast(c=>c.name==='natapp_save').args.value,'authToken')`,
      ),
      false,
    );
    run("click", 'button[title="删除隧道"]');
    wait();
    assert.equal(ev(`document.querySelectorAll('.natapp-row').length`), 1);
    click("取消");
    wait();
    assert.equal(ev(`document.querySelectorAll('.natapp-row').length`), 1);
    run("click", 'button[title="删除隧道"]');
    wait();
    click("确认删除");
    wait();
    assert.equal(ev(`document.querySelectorAll('.natapp-row').length`), 0);
    run("open", `${origin}/tests/tools.html?page=/tools/natapp&preview=1`);
    wait();
    assert.match(ev(`document.body.textContent`), /请在栖点桌面应用中使用/);
    console.log("工具页面 20 项模拟浏览器验收通过");
  }
} finally {
  run("close");
}
