import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function updaterConfig(repository, publicKey) {
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository || ""))
    throw Error("请设置 PERCH_RELEASE_REPOSITORY=所有者/仓库名");
  const key = publicKey?.trim();
  if (!key)
    throw Error("请设置 TAURI_UPDATER_PUBLIC_KEY，不能跳过更新签名校验");
  const decoded = Buffer.from(key, "base64").toString("utf8");
  if (!decoded.startsWith("untrusted comment:") || !decoded.includes("\n"))
    throw Error(
      "更新公钥格式无效，请使用 tauri signer generate 生成的 .pub 文件内容",
    );
  return {
    bundle: { createUpdaterArtifacts: true },
    plugins: {
      updater: {
        pubkey: key,
        endpoints: [
          `https://github.com/${repository}/releases/latest/download/latest.json`,
        ],
      },
    },
  };
}
if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    const config = updaterConfig(
      process.env.PERCH_RELEASE_REPOSITORY || process.env.GITHUB_REPOSITORY,
      process.env.TAURI_UPDATER_PUBLIC_KEY,
    );
    mkdirSync("src-tauri", { recursive: true });
    writeFileSync(
      "src-tauri/tauri.release.conf.json",
      JSON.stringify(config, null, 2) + "\n",
    );
    console.log("在线更新配置已生成，签名公钥已嵌入构建配置");
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
