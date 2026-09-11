// 由 node --test 单独运行，避免与前端 Vitest 的 *.test.* 收集规则冲突。
import test from "node:test";
import assert from "node:assert/strict";
import {
  mkdtempSync,
  writeFileSync,
  mkdirSync,
  rmSync,
  readFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import {
  collectCommits,
  releaseNotes,
  stableVersion,
  checkVersions,
} from "./release.mjs";
import { updaterConfig } from "./configure-updater.mjs";
import { verifyUpdaterSignature } from "./update-signature.mjs";
import { releaseArtifacts } from "./publish-release.mjs";
import { fileURLToPath } from "node:url";

function keysAndSignature(bytes) {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const id = Buffer.from("0123456789abcdef", "hex");
  const rawPublic = publicKey
    .export({ format: "der", type: "spki" })
    .subarray(-32);
  const keyPacket = Buffer.concat([Buffer.from("Ed"), id, rawPublic]);
  const signature = sign(
    null,
    createHash("blake2b512").update(bytes).digest(),
    privateKey,
  );
  const packet = Buffer.concat([Buffer.from("ED"), id, signature]);
  const comment = "timestamp:123456";
  const global = sign(
    null,
    Buffer.concat([signature, Buffer.from(comment)]),
    privateKey,
  );
  return {
    publicKey: Buffer.from(
      `untrusted comment: 测试公钥\n${keyPacket.toString("base64")}\n`,
    ).toString("base64"),
    signature: Buffer.from(
      `untrusted comment: 测试签名\n${packet.toString("base64")}\ntrusted comment: ${comment}\n${global.toString("base64")}\n`,
    ).toString("base64"),
  };
}
test("官方 Tauri 签名可被发布校验器验证", (t) => {
  const directory = mkdtempSync(join(tmpdir(), "perch-signer-test-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const key = join(directory, "temporary.key"),
    file = join(directory, "fixture.bin");
  const cli = fileURLToPath(
    new URL("../node_modules/@tauri-apps/cli/tauri.js", import.meta.url),
  );
  writeFileSync(file, "签名兼容测试");
  execFileSync(
    process.execPath,
    [cli, "signer", "generate", "--ci", "-p", "", "-w", key],
    { stdio: "pipe" },
  );
  execFileSync(
    process.execPath,
    [cli, "signer", "sign", "-f", key, "-p", "", file],
    { stdio: "pipe" },
  );
  assert.equal(
    verifyUpdaterSignature(
      readFileSync(file),
      readFileSync(`${file}.sig`, "utf8"),
      readFileSync(`${key}.pub`, "utf8"),
    ),
    true,
  );
});
test("稳定版本与当前元数据一致", () => {
  assert.equal(checkVersions(), stableVersion(checkVersions()));
  for (const invalid of ["v1.2.3", "1.2", "1.2.3-beta", "01.2.3"])
    assert.throws(() => stableVersion(invalid));
});
test("中文发布说明汇总所有提交并按类型分类", () => {
  const notes = releaseNotes("v1.2.3", [
    { hash: "abc", subject: "新增：在线更新" },
    { hash: "def", subject: "fix: 修复日历" },
    { hash: "ghi", subject: "优化：深色菜单" },
  ]);
  assert.match(notes, /新增功能/);
  assert.match(notes, /问题修复/);
  assert.match(notes, /体验优化/);
  assert.match(notes, /abc/);
  assert.match(notes, /def/);
  assert.match(notes, /ghi/);
  assert.throws(
    () => releaseNotes("v1.2.3", [{ hash: "abc", subject: "fix calendar" }]),
    /中文/,
  );
  assert.throws(() => releaseNotes("v1.2.3", []));
});
test("从祖先版本 Tag 到当前 Tag 汇总多个 commit", (t) => {
  const directory = mkdtempSync(join(tmpdir(), "perch-release-test-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const git = (...args) =>
    execFileSync("git", args, {
      cwd: directory,
      encoding: "utf8",
      stdio: "pipe",
    }).trim();
  git("init", "-b", "main");
  git("config", "user.name", "发布测试");
  git("config", "user.email", "test@example.invalid");
  git("commit", "--allow-empty", "-m", "新增：初始版本");
  git("tag", "v1.0.0");
  git("commit", "--allow-empty", "-m", "修复：第一项修复");
  git("commit", "--allow-empty", "-m", "新增：第二项功能");
  git("tag", "v1.1.0");
  const result = collectCommits("v1.1.0", directory);
  assert.equal(result.previous, "v1.0.0");
  assert.equal(result.commits.length, 2);
  assert.equal(result.commits[0].subject, "修复：第一项修复");
  assert.equal(collectCommits("v1.0.0", directory).commits.length, 1);
});
test("更新公钥与安装包签名必须匹配，篡改被拒绝", () => {
  const bytes = Buffer.from("更新包测试");
  const fixture = keysAndSignature(bytes);
  assert.equal(
    verifyUpdaterSignature(bytes, fixture.signature, fixture.publicKey),
    true,
  );
  assert.throws(() =>
    verifyUpdaterSignature(
      Buffer.from("被篡改"),
      fixture.signature,
      fixture.publicKey,
    ),
  );
  assert.throws(() =>
    verifyUpdaterSignature(
      bytes,
      fixture.signature,
      keysAndSignature(bytes).publicKey,
    ),
  );
  assert.throws(() =>
    verifyUpdaterSignature(bytes, "无效签名", fixture.publicKey),
  );
});
test("构建配置使用 HTTPS 和公钥，不接受空签名配置", () => {
  const key = keysAndSignature(Buffer.from("fixture")).publicKey;
  const config = updaterConfig("example/perch", key);
  assert.equal(
    config.plugins.updater.endpoints[0],
    "https://github.com/example/perch/releases/latest/download/latest.json",
  );
  assert.equal(config.bundle.createUpdaterArtifacts, true);
  assert.throws(() => updaterConfig("example/perch", ""));
  assert.throws(() => updaterConfig("https://example.com", key));
});
test("发布前必须同时存在两个平台产物和有效签名", async (t) => {
  const root = mkdtempSync(join(tmpdir(), "perch-artifacts-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const mac = join(root, "installers-aarch64-apple-darwin"),
    win = join(root, "installers-x86_64-pc-windows-msvc");
  mkdirSync(mac);
  mkdirSync(win);
  const bytes = Buffer.from("测试产物"),
    fixture = keysAndSignature(bytes);
  writeFileSync(join(mac, "栖点.app.tar.gz"), bytes);
  writeFileSync(join(mac, "栖点.app.tar.gz.sig"), fixture.signature);
  writeFileSync(join(mac, "栖点.dmg"), bytes);
  writeFileSync(join(win, "栖点.exe"), bytes);
  writeFileSync(join(win, "栖点.exe.sig"), fixture.signature);
  const result = await releaseArtifacts(root, "1.2.3", fixture.publicKey);
  assert.deepEqual(
    result.filter((a) => a.platform).map((a) => a.platform),
    ["darwin-aarch64", "windows-x86_64"],
  );
  assert.ok(result.every((a) => /^[\x00-\x7F]+$/.test(a.name)));
  writeFileSync(join(win, "栖点.exe"), Buffer.from("损坏"));
  await assert.rejects(() =>
    releaseArtifacts(root, "1.2.3", fixture.publicKey),
  );
});
