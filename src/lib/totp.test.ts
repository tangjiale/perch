import { describe, expect, it } from "vitest";
import { generateTotp, parseTotp, totpTiming } from "./totp";
function base32(text: string) {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let result = "",
    bits = 0,
    buffer = 0;
  for (const byte of new TextEncoder().encode(text)) {
    buffer = (buffer << 8) | byte;
    bits += 8;
    while (bits >= 5) {
      bits -= 5;
      result += alphabet[(buffer >>> bits) & 31];
    }
    buffer &= (1 << bits) - 1;
  }
  if (bits) result += alphabet[(buffer << (5 - bits)) & 31];
  return result;
}
describe("TOTP", () => {
  const times = [
    59, 1111111109, 1111111111, 1234567890, 2000000000, 20000000000,
  ];
  const vectors = {
    SHA1: [
      "94287082",
      "07081804",
      "14050471",
      "89005924",
      "69279037",
      "65353130",
    ],
    SHA256: [
      "46119246",
      "68084774",
      "67062674",
      "91819424",
      "90698825",
      "77737706",
    ],
    SHA512: [
      "90693936",
      "25091201",
      "99943326",
      "93441116",
      "38618901",
      "47863826",
    ],
  };
  const seeds = {
    SHA1: "12345678901234567890",
    SHA256: "12345678901234567890123456789012",
    SHA512: "1234567890123456789012345678901234567890123456789012345678901234",
  };
  for (const hash of Object.keys(vectors) as (keyof typeof vectors)[])
    for (const [index, seconds] of times.entries()) {
      it(`RFC 6238 ${hash} at ${seconds}`, async () => {
        const config = parseTotp(
          `otpauth://totp/RFC?secret=${base32(seeds[hash])}&algorithm=${hash}&digits=8`,
        );
        expect(await generateTotp(config, seconds * 1000)).toBe(
          vectors[hash][index],
        );
      });
    }
  it("accepts formatted lower-case secrets and keeps default settings", async () => {
    const config = parseTotp("gezd gnbv gy3t qojq gezd gnbv gy3t qojq");
    expect(config.period).toBe(30);
    expect(config.digits).toBe(6);
    expect(await generateTotp(config, 59000)).toBe("287082");
  });
  it("uses the next counter exactly at the time boundary", () => {
    expect(totpTiming(30, 29999).counter).toBe(0);
    expect(totpTiming(30, 30000)).toEqual({
      counter: 1,
      remaining: 30,
      progress: 1,
    });
  });
  it("supports custom periods and labels", () => {
    const config = parseTotp(
      "otpauth://totp/Example%3Aalice?secret=MY======&period=60&digits=8",
    );
    expect(config.label).toBe("Example:alice");
    expect(config.period).toBe(60);
  });
  for (const input of [
    "",
    "hello1",
    "A",
    "MZ",
    "MY=",
    "otpauth://hotp/account?secret=MY",
    "otpauth://totp/a?secret=MY&algorithm=MD5",
    "otpauth://totp/a?secret=MY&digits=7",
    "otpauth://totp/a?secret=MY&period=0",
    "otpauth://totp/a?secret=MY&period=3.5",
  ])
    it(`rejects invalid input ${input}`, () =>
      expect(() => parseTotp(input)).toThrow());
});
