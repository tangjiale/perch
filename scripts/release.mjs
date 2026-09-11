import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parse } from "smol-toml";

export const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export function stableVersion(value) {
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(value))
    throw Error("仅支持 X.Y.Z 格式的稳定版本");
  return value;
}
const json = (path) => JSON.parse(readFileSync(resolve(root, path), "utf8"));
function git(args, cwd = root) {
  return execFileSync("git", args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}
export function collectCommits(tag, cwd = root) {
  const version = stableVersion(tag.replace(/^v/, ""));
  if (tag !== `v${version}`) throw Error("Tag 必须使用 vX.Y.Z");
  const target = git(["rev-parse", `${tag}^{commit}`], cwd);
  const tags = git(["tag", "--merged", target, "--sort=-version:refname"], cwd)
    .split("\n")
    .filter((name) => /^v\d+\.\d+\.\d+$/.test(name) && name !== tag);
  const compare = (a, b) => {
    const av = a.slice(1).split(".").map(Number),
      bv = b.slice(1).split(".").map(Number);
    return av[0] - bv[0] || av[1] - bv[1] || av[2] - bv[2];
  };
  if (tags.some((previous) => compare(previous, tag) > 0))
    throw Error("不能发布低于现有祖先版本的稳定 Tag");
  const previous = tags.find((name) => compare(name, tag) < 0);
  const range = previous ? `${previous}..${target}` : target;
  const rows = git(
    ["log", "--reverse", "--no-merges", "--format=%h%x09%s", range],
    cwd,
  );
  const commits = rows
    ? rows.split("\n").map((line) => ({
        hash: line.slice(0, line.indexOf("\t")),
        subject: line.slice(line.indexOf("\t") + 1),
      }))
    : [];
  return { previous, commits };
}
export function releaseNotes(tag, commits) {
  if (!commits.length) throw Error("当前 Tag 没有可汇总的新提交");
  const invalid = commits.filter(
    (commit) => !/\p{Script=Han}/u.test(commit.subject),
  );
  if (invalid.length)
    throw Error(
      `以下提交缺少中文说明：${invalid.map((c) => c.hash).join("、")}`,
    );
  const groups = new Map([
    ["新增功能", []],
    ["问题修复", []],
    ["体验优化", []],
    ["工程与维护", []],
  ]);
  for (const { hash, subject } of commits) {
    const group = /^(feat|新增|功能)(\b|[：:(])/iu.test(subject)
      ? "新增功能"
      : /^(fix|修复)(\b|[：:(])/iu.test(subject)
        ? "问题修复"
        : /^(perf|style|优化|体验)(\b|[：:(])/iu.test(subject)
          ? "体验优化"
          : "工程与维护";
    groups.get(group).push(`- ${subject.replace(/[\r\n]/g, " ")}（${hash}）`);
  }
  return [
    `# 栖点 ${tag}`,
    "",
    ...[...groups]
      .filter(([, rows]) => rows.length)
      .flatMap(([name, rows]) => [`## ${name}`, "", ...rows, ""]),
  ].join("\n");
}
export function checkVersions(tag) {
  const version = stableVersion(json("package.json").version);
  const lock = json("package-lock.json");
  const tauri = json("src-tauri/tauri.conf.json");
  const cargo = parse(
    readFileSync(resolve(root, "src-tauri/Cargo.toml"), "utf8"),
  );
  const cargoLock = parse(
    readFileSync(resolve(root, "src-tauri/Cargo.lock"), "utf8"),
  );
  const versions = [
    lock.version,
    lock.packages[""].version,
    tauri.version,
    cargo.package.version,
    cargoLock.package.find((p) => p.name === cargo.package.name)?.version,
  ];
  if (versions.some((value) => value !== version))
    throw Error(
      "版本不一致，请先运行 npm run release:prepare -- X.Y.Z 并提交修改",
    );
  if (tag && tag !== `v${version}`)
    throw Error(`Tag ${tag} 与应用版本 v${version} 不一致`);
  return version;
}
function updateTomlPackage(path, name, version) {
  const text = readFileSync(resolve(root, path), "utf8");
  const original = parse(text);
  const lock = Array.isArray(original.package);
  const pattern = lock
    ? /\[\[package\]\][\s\S]*?(?=\n\[\[package\]\]|$)/g
    : /\[package\][\s\S]*?(?=\n\[|$)/g;
  let edits = 0;
  const next = text.replace(pattern, (block) => {
    const data = parse(block).package;
    if ((lock ? data[0] : data).name !== name) return block;
    edits++;
    return block.replace(/^(version\s*=\s*)"[^"]+"/m, `$1"${version}"`);
  });
  if (edits !== 1) throw Error(`无法唯一定位 ${path} 的包版本`);
  const expected = structuredClone(original);
  (lock
    ? expected.package.find((p) => p.name === name)
    : expected.package
  ).version = version;
  if (JSON.stringify(parse(next)) !== JSON.stringify(expected))
    throw Error(`${path} 出现非版本变更`);
  return next;
}
function prepare(version) {
  stableVersion(version);
  const pkg = json("package.json"),
    lock = json("package-lock.json"),
    tauri = json("src-tauri/tauri.conf.json");
  const name = parse(
    readFileSync(resolve(root, "src-tauri/Cargo.toml"), "utf8"),
  ).package.name;
  const cargo = updateTomlPackage("src-tauri/Cargo.toml", name, version);
  const cargoLock = updateTomlPackage("src-tauri/Cargo.lock", name, version);
  pkg.version =
    lock.version =
    lock.packages[""].version =
    tauri.version =
      version;
  for (const [path, value] of [
    ["package.json", pkg],
    ["package-lock.json", lock],
    ["src-tauri/tauri.conf.json", tauri],
  ])
    writeFileSync(resolve(root, path), JSON.stringify(value, null, 2) + "\n");
  writeFileSync(resolve(root, "src-tauri/Cargo.toml"), cargo);
  writeFileSync(resolve(root, "src-tauri/Cargo.lock"), cargoLock);
  console.log(`版本已同步为 ${version}，请检查并用中文提交这些修改。`);
}
export function main(args) {
  const [action, ...rest] = args;
  const option = (flag) => {
    const index = rest.indexOf(flag);
    return index >= 0 ? rest[index + 1] : undefined;
  };
  if (action === "prepare") {
    prepare(rest[0]);
    return;
  }
  if (action === "check") {
    console.log(`版本一致：${checkVersions(option("--tag"))}`);
    return;
  }
  if (action === "notes") {
    const tag = option("--tag");
    if (!tag) throw Error("请指定 --tag vX.Y.Z");
    checkVersions(tag);
    const { commits } = collectCommits(tag);
    const notes = releaseNotes(tag, commits);
    const output = resolve(root, option("--output") || ".release/notes.md");
    mkdirSync(dirname(output), { recursive: true });
    writeFileSync(output, notes);
    console.log(`已汇总 ${commits.length} 个提交的中文更新说明`);
    return;
  }
  if (action === "tag") {
    const version = checkVersions();
    if (git(["status", "--porcelain"]))
      throw Error("工作区有未提交修改，不能创建发布 Tag");
    const tag = `v${version}`;
    // 本地创建带中文说明的 Tag，不隐式推送到远端。
    const existing = git(["tag", "--list", tag]);
    if (existing) throw Error(`Tag ${tag} 已存在，请勿覆盖已发布版本`);
    const target = git(["rev-parse", "HEAD"]);
    const previous = git(["tag", "--merged", "HEAD", "--sort=-version:refname"])
      .split("\n")
      .find((t) => /^v\d+\.\d+\.\d+$/.test(t));
    if (previous) {
      const before = previous.slice(1).split(".").map(Number),
        next = version.split(".").map(Number);
      const order =
        next[0] - before[0] || next[1] - before[1] || next[2] - before[2];
      if (order <= 0) throw Error("新 Tag 必须高于现有祖先版本");
    }
    const rows = git([
      "log",
      "--reverse",
      "--no-merges",
      "--format=%h%x09%s",
      previous ? `${previous}..HEAD` : "HEAD",
    ]);
    const commits = rows
      ? rows.split("\n").map((row) => ({
          hash: row.split("\t")[0],
          subject: row.slice(row.indexOf("\t") + 1),
        }))
      : [];
    const notes = releaseNotes(tag, commits);
    mkdirSync(resolve(root, ".release"), { recursive: true });
    const body = resolve(root, ".release/tag-notes.md");
    writeFileSync(body, notes);
    git(["tag", "-a", tag, target, "-F", body]);
    console.log(
      `已创建 ${tag}，说明汇总 ${commits.length} 个提交。确认后执行 git push origin ${tag}。`,
    );
    return;
  }
  throw Error(
    "用法：release.mjs prepare X.Y.Z | check [--tag vX.Y.Z] | notes --tag vX.Y.Z [--output 路径] | tag",
  );
}
if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
