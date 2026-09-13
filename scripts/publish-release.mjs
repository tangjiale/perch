import { readFile, readdir } from "node:fs/promises";
import { resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  normalizePublicKey,
  verifyUpdaterSignature,
} from "./update-signature.mjs";

async function filesAt(root) {
  const result = [];
  for (const entry of await readdir(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) result.push(...(await filesAt(path)));
    else if (entry.isFile()) result.push(path);
  }
  return result;
}
export async function releaseArtifacts(root, version, publicKey) {
  const paths = await filesAt(root);
  const unique = (target, suffix) => {
    const matches = paths.filter(
      (path) => path.includes(`installers-${target}`) && path.endsWith(suffix),
    );
    if (matches.length !== 1)
      throw Error(`${target} 必须有且仅有一个 ${suffix} 产物`);
    return matches[0];
  };
  const mac = unique("aarch64-apple-darwin", ".app.tar.gz");
  const win = unique("x86_64-pc-windows-msvc", ".exe");
  const dmg = unique("aarch64-apple-darwin", ".dmg");
  const artifacts = [];
  for (const [path, name, platform] of [
    [mac, `Perch_${version}_aarch64.app.tar.gz`, "darwin-aarch64"],
    [win, `Perch_${version}_x64-setup.exe`, "windows-x86_64"],
  ]) {
    const bytes = await readFile(path);
    const signature = (await readFile(`${path}.sig`, "utf8")).trim();
    verifyUpdaterSignature(bytes, signature, publicKey);
    artifacts.push(
      { name, bytes, signature, platform },
      { name: `${name}.sig`, bytes: Buffer.from(signature) },
    );
  }
  artifacts.push({
    name: `Perch_${version}_aarch64.dmg`,
    bytes: await readFile(dmg),
  });
  return artifacts;
}
export async function publish() {
  const repository = process.env.GITHUB_REPOSITORY;
  const tag = process.env.GITHUB_REF_NAME;
  const token = process.env.GITHUB_TOKEN;
  const configuredPublicKey = process.env.TAURI_UPDATER_PUBLIC_KEY;
  if (
    !/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository || "") ||
    !/^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(tag || "")
  )
    throw Error("仓库或稳定版本 Tag 无效");
  if (!token || !configuredPublicKey)
    throw Error("缺少发布凭据或更新签名公钥");
  const publicKey = normalizePublicKey(configuredPublicKey);
  const version = tag.slice(1);
  const notes = await readFile("release-notes.md", "utf8");
  if (!notes.startsWith(`# 栖点 ${tag}\n`) || !/\p{Script=Han}/u.test(notes))
    throw Error("更新说明与 Tag 不一致");
  const artifacts = await releaseArtifacts(
    resolve("artifacts"),
    version,
    publicKey,
  );
  const headers = {
    Authorization: `Bearer ${token}`,
    Accept: "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28",
  };
  const base = `https://api.github.com/repos/${repository}`;
  async function api(url, options = {}, allow404 = false) {
    const response = await fetch(url, {
      ...options,
      headers: { ...headers, ...options.headers },
      signal: AbortSignal.timeout(180_000),
    });
    if (allow404 && response.status === 404) return null;
    if (!response.ok)
      throw Error(`GitHub 发布请求失败：HTTP ${response.status}`);
    return response.status === 204 ? null : response.json();
  }
  let release = await api(`${base}/releases/tags/${tag}`, {}, true);
  if (release && !release.draft)
    throw Error("该版本已公开发布，禁止覆盖；请创建更高版本的 Tag");
  const latestRelease = await api(`${base}/releases/latest`, {}, true);
  if (latestRelease && /^v\d+\.\d+\.\d+$/.test(latestRelease.tag_name)) {
    const previous = latestRelease.tag_name.slice(1).split(".").map(Number),
      next = version.split(".").map(Number);
    const order =
      next[0] - previous[0] || next[1] - previous[1] || next[2] - previous[2];
    if (order <= 0)
      throw Error("版本必须高于当前公开 Release，禁止更新清单降级");
  }
  const body = {
    tag_name: tag,
    name: `栖点 ${tag}`,
    body: notes,
    draft: true,
    prerelease: false,
  };
  const request = {
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  };
  release = release
    ? await api(`${base}/releases/${release.id}`, {
        ...request,
        method: "PATCH",
      })
    : await api(`${base}/releases`, { ...request, method: "POST" });
  const upload = release.upload_url.replace(/\{.*$/, "");
  if (!upload.startsWith(`https://uploads.github.com/repos/${repository}/`))
    throw Error("GitHub 上传地址无效");
  const existing = await api(
    `${base}/releases/${release.id}/assets?per_page=100`,
  );
  async function uploadAsset(name, bytes) {
    const old = existing.find((asset) => asset.name === name);
    if (old)
      await api(`${base}/releases/assets/${old.id}`, { method: "DELETE" });
    return api(`${upload}?name=${encodeURIComponent(name)}`, {
      method: "POST",
      headers: { "Content-Type": "application/octet-stream" },
      body: bytes,
    });
  }
  const platforms = {};
  for (const artifact of artifacts) {
    const asset = await uploadAsset(artifact.name, artifact.bytes);
    if (artifact.platform)
      platforms[artifact.platform] = {
        signature: artifact.signature,
        // GitHub 在草稿 Release 上传阶段可能返回 untagged 临时路径；
        // 发布后该路径会失效，更新清单必须使用稳定的 Tag 下载地址。
        url: `https://github.com/${repository}/releases/download/${tag}/${encodeURIComponent(artifact.name)}`,
      };
  }
  const manifest = {
    version,
    notes,
    pub_date: new Date().toISOString(),
    platforms,
  };
  await uploadAsset(
    "latest.json",
    Buffer.from(JSON.stringify(manifest, null, 2) + "\n"),
  );
  const published = await api(`${base}/releases/${release.id}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ draft: false, make_latest: "true" }),
  });
  let verified = false;
  for (let attempt = 0; attempt < 6; attempt++) {
    try {
      const live = await fetch(
        `https://github.com/${repository}/releases/latest/download/latest.json`,
        {
          signal: AbortSignal.timeout(15_000),
          headers: { "Cache-Control": "no-cache" },
        },
      );
      if (live.ok) {
        const value = await live.json();
        verified =
          value.version === version &&
          Object.entries(platforms).every(
            ([platform, entry]) =>
              value.platforms?.[platform]?.signature === entry.signature &&
              value.platforms?.[platform]?.url === entry.url,
          );
        if (verified) break;
      }
    } catch {
      /* GitHub 公开资产可能短暂传播延迟，有限重试。 */
    }
    if (attempt < 5) await new Promise((resolve) => setTimeout(resolve, 5000));
  }
  if (!verified)
    throw Error(
      "Release 已发布，但公开更新清单尚未验证通过，请单独检查公开地址，不要覆盖 Tag",
    );
  console.log(`已发布栖点 ${tag}：${published.html_url}`);
}
if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  publish().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
