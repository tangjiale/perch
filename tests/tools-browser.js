// 仅模拟 IPC，不操作真实 NATAPP 或凭据。
import { execFileSync } from "node:child_process";
import assert from "node:assert/strict";
const origin = process.argv[2] || "http://127.0.0.1:1421";
const scenario = process.argv[3]?.replace(/^--/, "") || "all";
const run = (...args) =>
  execFileSync(
    "npx",
    ["--yes", "agent-browser", "--session", `tools-test-${scenario}`, ...args],
    { encoding: "utf8", timeout: 60000 },
  );
const ev = (code) =>
  JSON.parse(JSON.parse(run("eval", `JSON.stringify(${code})`).trim()));
const wait = () => run("wait", "250");
const click = (text) =>
  ev(
    `(()=>{const b=[...document.querySelectorAll('button')].find(x=>x.textContent.trim()===${JSON.stringify(text)});if(!b)throw Error('button missing');b.click();return true})()`,
  );
const disabled = (text) =>
  ev(`[...document.querySelectorAll('.natapp-install button')].find(b=>b.textContent.trim()===${JSON.stringify(text)})?.disabled ?? null`);
const waitForPanel = (text) =>
  run("wait", "--fn", `document.querySelector('.natapp-install')?.textContent.includes(${JSON.stringify(text)})`);
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
    waitForPanel("请先连接 NATAPP 账号");
    assert.equal(disabled("下载并安装"), true, "未绑定时禁用在线安装");
    assert.equal(disabled("从文件安装"), false, "未绑定时仍可从文件安装");
    assert.equal(ev(`window.__calls.some(c=>c.name==='natapp_client_release')`), false);
    run("open", `${origin}/tests/tools.html?page=/tools/natapp&bound=1`);
    run("wait", "--fn", "document.body.textContent.includes('还没有隧道')");
    run("wait", "--fn", "document.querySelector('.natapp-install button.primary')?.disabled === false");
    assert.equal(disabled("下载并安装"), false, "绑定账号但没有隧道也可以下载");
    ev(
      `(() => { const original=window.__TAURI_INTERNALS__.invoke;let failing=true;window.__TAURI_INTERNALS__.invoke=async(name,args)=>{if(name==='natapp_install'&&failing){window.__state.installing=true;return new Promise((resolve,reject)=>window.__rejectInstall=()=>{failing=false;window.__state.installing=false;reject('NATAPP 官网要求登录后下载（HTTP 403）。请打开「NATAPP 官网下载」登录并获取安装包，再选择「从文件安装」或「从文件更新」。');});}if(name==='natapp_state'&&window.__state.installing){const old=structuredClone(window.__state);return new Promise(resolve=>window.__resolveOldInstalling=()=>resolve(old));}return original(name,args);};return true;})()`,
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
      /NATAPP 官网要求登录后下载（HTTP 403）。.*「从文件安装」/,
      "轮询保留官网登录与文件安装提示",
    );
    click("下载并安装");
    wait();
    assert.match(
      ev(`document.body.textContent`),
      /客户端已安装/,
      "失败后可重新安装成功",
    );
    assert.equal(
      ev(`window.__calls.findLast(c=>c.name==='natapp_install').args?.update === undefined`),
      true,
      "初装不传更新标记",
    );
    run("open", `${origin}/tests/tools.html?page=/tools/natapp`);
    run("wait", "--fn", "document.body.textContent.includes('还没有隧道')");
    ev(`window.__packagePath='/mock/natapp.zip'`);
    click("从文件安装");
    run("wait", "--fn", "document.body.textContent.includes('客户端已安装')");
    assert.equal(
      ev(`window.__calls.findLast(c=>c.name==='natapp_install').args.packagePath`),
      "/mock/natapp.zip",
    );
    assert.equal(
      ev(`window.__calls.some(c=>['natapp_save','natapp_sync_tunnels'].includes(c.name))`),
      false,
      "从文件安装无需添加或同步隧道",
    );
    console.log("NATAPP 无隧道安装、失败恢复与本地文件安装验收通过");
    process.exitCode = 0;
  } else if (process.argv.includes("--client-update")) {
    const page = `${origin}/tests/tools.html?page=/tools/natapp`;
    run("open", `${page}&bound=1&installed=3.0.9`);
    waitForPanel("发现新版 3.0.10");
    assert.equal(disabled("下载并更新"), false);
    assert.equal(ev(`window.__calls.some(c=>c.name==='natapp_install')`), false, "自动检查不安装");
    const checks = ev(`window.__calls.filter(c=>c.name==='natapp_client_release').length`);
    click("检查更新");
    waitForPanel("发现新版 3.0.10");
    assert.equal(ev(`window.__calls.filter(c=>c.name==='natapp_client_release').length`), checks + 1);
    click("下载并更新");
    waitForPanel("已是最新版本");
    assert.equal(ev(`window.__calls.findLast(c=>c.name==='natapp_install').args.update`), true);
    assert.equal(disabled("下载并更新"), null);

    run("open", `${page}&bound=1&installed=3.1.2&latest=3.0.99`);
    waitForPanel("官网版本 3.0.99");
    assert.equal(disabled("下载并更新"), null, "官网版本更低时不提供降级");
    ev(`window.__releaseError='模拟检查失败'`);
    click("检查更新");
    waitForPanel("检查更新失败 · 模拟检查失败");
    assert.equal(disabled("从文件更新"), false, "版本检查失败时仍可手动更新");
    ev(`(delete window.__releaseError, true)`);
    click("检查更新");
    waitForPanel("官网版本 3.0.99");

    run("open", `${page}&bound=1&installed=&active=1`);
    waitForPanel("版本未知");
    waitForPanel("官网版本 3.0.10");
    waitForPanel("请先停止运行中的隧道");
    assert.equal(disabled("安装官网版"), true);
    assert.equal(disabled("从文件更新"), true);
    click("停止");
    run("wait", "--fn", "document.querySelector('.natapp-install button.primary')?.disabled === false");
    click("安装官网版");
    waitForPanel("已是最新版本");
    assert.equal(ev(`window.__calls.findLast(c=>c.name==='natapp_install').args.update`), true);

    run("open", `${page}&installed=3.0.9&windows=1`);
    waitForPanel("请先连接 NATAPP 账号");
    waitForPanel("发现新版 3.0.10");
    assert.equal(disabled("下载并更新"), true);
    assert.equal(disabled("从文件更新"), false);
    ev(`window.__packagePath='/mock/natapp.zip'`);
    click("从文件更新");
    waitForPanel("版本未知");
    assert.equal(ev(`window.__notice`), "客户端更新完成");
    assert.equal(disabled("安装官网版"), true);
    assert.deepEqual(ev(`window.__calls.findLast(c=>c.name==='natapp_install').args`), {
      packagePath: "/mock/natapp.zip",
    });
    assert.deepEqual(ev(`window.__calls.findLast(c=>c.name==='plugin:dialog|open').args.options.filters[0].extensions`), ["zip"]);
    console.log("NATAPP 版本检查、在线更新和无账号覆盖安装验收通过");
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
    run("open", `${origin}/tests/tools.html?page=/tools/natapp&bound=1`);
    wait();
    assert.equal(
      ev(`document.querySelector('.natapp-install button.primary').disabled`),
      false,
    );
    click("手动添加");
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
