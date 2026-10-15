const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const context = { module: {exports:{}}, URL, AbortController, setTimeout, clearTimeout };
vm.runInNewContext(fs.readFileSync(require('node:path').join(__dirname, '../docs/main.js'), 'utf8'), context);
const { releaseAssetUrl, releaseInfo, fetchRelease } = context.module.exports;
const base = 'https://github.com/tangjiale/perch/releases/download/v0.1.19/';
const release = {tag_name:'v0.1.19', draft:false, prerelease:false, published_at:'2026-10-15T00:00:00Z', assets:[{browser_download_url:base+'Perch_0.1.19_aarch64.dmg'},{browser_download_url:base+'Perch_0.1.19_x64-setup.exe'}]};
test('正式 Release 使用实际安装包，不生成不存在的链接', () => {
  const info = releaseInfo(release);
  assert.equal(info.mac, base+'Perch_0.1.19_aarch64.dmg');
  assert.equal(info.win, base+'Perch_0.1.19_x64-setup.exe');
  assert.equal(releaseInfo({...release,assets:[]}).mac,null);
});
test('拒绝预发布、错误版本、外部资产与危险地址', () => {
  assert.throws(()=>releaseInfo({...release,prerelease:true}));
  assert.throws(()=>releaseInfo({...release,draft:true}));
  assert.throws(()=>releaseInfo({...release,tag_name:'v0.1.19-beta'}));
  for (const url of ['javascript:alert(1)',base.replace('github.com','evil.test')+'a.dmg',base.replace('v0.1.19','v0.1.18')+'a.dmg',base+'a.dmg?redirect=1']) assert.equal(releaseAssetUrl(url,'v0.1.19'),null);
  assert.equal(releaseInfo({...release,assets:[{browser_download_url:base+'Perch_0.1.18_aarch64.dmg'}]}).mac,null);
});
test('API 优先且不缓存，旧静态清单不覆盖实时结果', async () => {
  const calls=[];
  const info=await fetchRelease(async(url,opts)=>{calls.push(url);assert.equal(opts.cache,'no-store');assert.ok(opts.signal);return {ok:true,json:async()=>release};});
  assert.equal(info.tag,'v0.1.19'); assert.equal(info.fallback,false); assert.equal(calls.length,1);
});
test('API 失败明确回退静态发布记录，只保留存在的资产', async () => {
  const info=await fetchRelease(async url=>{if(url.startsWith('https:'))throw Error('rate limit');return {ok:true,json:async()=>({version:'0.1.18',pub_date:'2026-10-12',platforms:{mac:{url:'https://github.com/tangjiale/perch/releases/download/v0.1.18/Perch_0.1.18_aarch64.app.tar.gz'}}})};});
  assert.equal(info.fallback,true);assert.equal(info.tag,'v0.1.18');assert.equal(info.mac,null);
});
test('两个来源都失败时向 UI 报错以保留 latest 通用入口', async () => {
  await assert.rejects(()=>fetchRelease(async()=>({ok:false})));
});
