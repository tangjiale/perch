import { createHash, createPublicKey, verify } from "node:crypto";

export function normalizePublicKey(publicKey) {
  let value = publicKey?.trim();
  for (let attempt = 0; attempt < 3 && value; attempt += 1) {
    if (value.startsWith("untrusted comment:") && value.includes("\n")) {
      return Buffer.from(value + (value.endsWith("\n") ? "" : "\n")).toString(
        "base64",
      );
    }
    const decoded = Buffer.from(value, "base64").toString("utf8");
    if (!decoded || decoded === value) break;
    value = decoded.trim();
  }
  throw Error(
    "更新公钥格式无效，请使用 tauri signer generate 生成的 .pub 文件内容",
  );
}

export function verifyUpdaterSignature(bytes, signature, publicKey) {
  const keyLines = Buffer.from(normalizePublicKey(publicKey), "base64")
    .toString("utf8")
    .trim()
    .split(/\r?\n/);
  const sigLines = Buffer.from(signature.trim(), "base64")
    .toString("utf8")
    .trim()
    .split(/\r?\n/);
  const key = Buffer.from(keyLines[1] || "", "base64");
  const packet = Buffer.from(sigLines[1] || "", "base64");
  if (
    key.length !== 42 ||
    packet.length !== 74 ||
    !packet.subarray(2, 10).equals(key.subarray(2, 10))
  )
    throw Error("更新签名与配置公钥不匹配");
  const algorithm = packet.subarray(0, 2).toString();
  if (
    !["Ed", "ED"].includes(algorithm) ||
    key.subarray(0, 2).toString() !== "Ed"
  )
    throw Error("不支持的更新签名格式");
  const publicObject = createPublicKey({
    key: Buffer.concat([
      Buffer.from("302a300506032b6570032100", "hex"),
      key.subarray(10),
    ]),
    format: "der",
    type: "spki",
  });
  const payload =
    algorithm === "ED"
      ? createHash("blake2b512").update(bytes).digest()
      : bytes;
  if (!verify(null, payload, publicObject, packet.subarray(10)))
    throw Error("更新包签名验证失败，禁止发布");
  if (!sigLines[2]?.startsWith("trusted comment: "))
    throw Error("更新签名缺少受信任注释");
  const comment = sigLines[2].slice("trusted comment: ".length);
  const signedComment = Buffer.concat([
    packet.subarray(10),
    Buffer.from(comment),
  ]);
  if (
    !verify(
      null,
      signedComment,
      publicObject,
      Buffer.from(sigLines[3] || "", "base64"),
    )
  )
    throw Error("更新签名注释验证失败");
  return true;
}
