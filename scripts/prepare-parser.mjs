import { createHash } from "node:crypto";
import { constants, createReadStream, createWriteStream } from "node:fs";
import { access, copyFile, cp, mkdir, mkdtemp, readFile, readdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { pipeline } from "node:stream/promises";
import { spawnSync } from "node:child_process";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const root = join(repo, "src-tauri/resources/parser");
const manifest = JSON.parse(await readFile(new URL("./parser-runtime.json", import.meta.url), "utf8"));
const platform = `${process.platform}-${process.arch}`;
const runtime = manifest.jre.platforms[platform];
if (!runtime) throw new Error(`文档解析运行时暂不支持 ${platform}`);
if (process.argv.slice(2).some((arg) => arg !== "--check")) throw new Error("仅支持 --check 参数");
const checkOnly = process.argv.includes("--check");

async function exists(path) {
  try { await access(path); return true; } catch (error) {
    if (error.code === "ENOENT") return false;
    throw error;
  }
}

async function checksum(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

async function verifyJava(path) {
  const home = join(path, runtime.home);
  const release = await readFile(join(home, "release"), "utf8");
  const values = Object.fromEntries(release.split(/\r?\n/).filter((line) => line.includes("=")).map((line) => {
    const index = line.indexOf("=");
    return [line.slice(0, index), line.slice(index + 1).replace(/^"|"$/g, "")];
  }));
  if (values.SEMANTIC_VERSION !== manifest.jre.version || values.OS_NAME !== runtime.osName || values.OS_ARCH !== runtime.osArch) {
    throw new Error("已有 JRE 的版本或架构不匹配；请在全新检出目录构建，脚本不会覆盖已有运行时");
  }
  await access(join(home, runtime.java));
  await access(join(home, "legal"));
}

async function download(asset, destination) {
  const response = await fetch(asset.url, { signal: AbortSignal.timeout(180_000) });
  if (!response.ok || !response.body) throw new Error(`运行时下载失败：HTTP ${response.status}`);
  await pipeline(response.body, createWriteStream(destination, { flags: "wx" }));
  if (await checksum(destination) !== asset.sha256) throw new Error("运行时 SHA-256 校验失败，禁止打包");
}

await mkdir(root, { recursive: true });
const jar = join(root, "tika-app.jar");
const jre = join(root, "jre");
const hasJar = await exists(jar);
const hasJre = await exists(jre);
if (hasJar && await checksum(jar) !== manifest.tika.sha256) throw new Error("已有 Tika SHA-256 不匹配，脚本不会覆盖该文件");
if (hasJre) await verifyJava(jre);
if (checkOnly && (!hasJar || !hasJre)) throw new Error("运行时缺失，请先运行 node scripts/prepare-parser.mjs");

if (!hasJar || !hasJre) {
  // 临时目录保留以便下载失败时排查；不修改或删除已有构建资源。
  const temp = await mkdtemp(join(tmpdir(), "perch-parser-"));
  if (!hasJar) {
    const stagedJar = join(temp, "tika-app.jar");
    await download(manifest.tika, stagedJar);
    await copyFile(stagedJar, jar, constants.COPYFILE_EXCL);
  }
  if (!hasJre) {
    const archive = join(temp, process.platform === "win32" ? "jre.zip" : "jre.tar.gz");
    await download(runtime, archive);
    const extracted = join(temp, "extracted");
    await mkdir(extracted);
    const extraction = process.platform === "win32"
      ? spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", "Expand-Archive -LiteralPath $env:PERCH_PARSER_ARCHIVE -DestinationPath $env:PERCH_PARSER_EXTRACT"], {
          stdio: "inherit", env: { ...process.env, PERCH_PARSER_ARCHIVE: archive, PERCH_PARSER_EXTRACT: extracted },
        })
      : spawnSync("tar", ["-xzf", archive, "-C", extracted], { stdio: "inherit" });
    if (extraction.error || extraction.status !== 0) throw new Error("JRE 解压失败");
    const entries = await readdir(extracted, { withFileTypes: true });
    if (entries.length !== 1 || !entries[0].isDirectory()) throw new Error("JRE 压缩包目录结构不符合预期");
    const stagedJre = join(extracted, entries[0].name);
    await verifyJava(stagedJre);
    await cp(stagedJre, jre, { recursive: true, force: false, errorOnExist: true, verbatimSymlinks: true });
  }
}
console.log(`文档解析运行时已校验：Tika ${manifest.tika.version}，Temurin ${manifest.jre.version} (${platform})`);
