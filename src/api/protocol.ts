// Pure protocol crypto for pairing and login (client_reqs.md §5–§8).
// No React Native imports, so it can be checked against the §11 test vectors
// outside the app. Relies on a global crypto.getRandomValues (polyfilled in
// the app by react-native-get-random-values, imported in src/app/_layout.tsx).

import { ed25519 } from "@noble/curves/ed25519.js";
import { hkdf } from "@noble/hashes/hkdf.js";
import { hmac } from "@noble/hashes/hmac.js";
import { sha256 } from "@noble/hashes/sha2.js";

// ---------- bytes helpers ----------

export const utf8 = (s: string): Uint8Array => new TextEncoder().encode(s);

export function concat(...parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let offset = 0;
  for (const p of parts) {
    out.set(p, offset);
    offset += p.length;
  }
  return out;
}

/** Standard base64 with padding. */
export function toBase64(bytes: Uint8Array): string {
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin);
}

export function fromBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

/** Constant-time comparison (§12: proofs compared in constant time). */
export function constantTimeEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a[i] ^ b[i];
  return diff === 0;
}

export function randomBytes(n: number): Uint8Array {
  return crypto.getRandomValues(new Uint8Array(n));
}

// ---------- §5 transcript and messages ----------

/** Each field: 4-byte big-endian length, then the bytes. */
export function transcript(...fields: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(fields.reduce((n, f) => n + 4 + f.length, 0));
  const view = new DataView(out.buffer);
  let offset = 0;
  for (const f of fields) {
    view.setUint32(offset, f.length, false);
    out.set(f, offset + 4);
    offset += 4 + f.length;
  }
  return out;
}

export const clientConfirm = (seenKeyHash: Uint8Array, publicKey: Uint8Array, deviceName: string) =>
  transcript(utf8("riftview-pair-v1 client-confirm"), seenKeyHash, publicKey, utf8(deviceName));

export const hostConfirm = (seenKeyHash: Uint8Array, publicKey: Uint8Array, deviceId: string) =>
  transcript(utf8("riftview-pair-v1 host-confirm"), seenKeyHash, publicKey, utf8(deviceId));

export const loginMessage = (pinHash: Uint8Array, deviceId: string, nonce: Uint8Array) =>
  transcript(utf8("riftview-login-v1"), pinHash, utf8(deviceId), nonce);

/** "sha256/" + base64(hash) — the pin text format the host uses. */
export const pinText = (pinHash: Uint8Array) => "sha256/" + toBase64(pinHash);

// ---------- §6 SPAKE2 (side A), byte-compatible with Rust spake2 0.4 ----------

const Point = ed25519.Point;
type EdPoint = InstanceType<typeof Point>;

const L = 2n ** 252n + 27742317777372353535851937790883648493n;
const M = Point.fromHex("15cfd18e385952982b6a8f8c7854963b58e34388c8e6dae891db756481a02312");
const N = Point.fromHex("f04f2e7eb734b2a8f8b472eaf9c3c632576ac64aea650b496a8a20ff00e583c3");
const ID_A = "riftview-client";
const ID_B = "riftview-host";

const bigIntFromBE = (b: Uint8Array) => b.reduce((acc, byte) => (acc << 8n) | BigInt(byte), 0n);

export function passwordScalar(code: string): bigint {
  const okm = hkdf(sha256, utf8(code), new Uint8Array(0), utf8("SPAKE2 pw"), 48);
  return bigIntFromBE(okm) % L;
}

function randomScalar(): bigint {
  let x = 0n;
  while (x === 0n) x = bigIntFromBE(randomBytes(64)) % L;
  return x;
}

export type Spake2State = { code: string; pw: bigint; x: bigint; X: Uint8Array };

/** `x` is only passed by tests (§11 fixed scalar); the app always uses a random one. */
export function spake2Start(code: string, x: bigint = randomScalar()): { state: Spake2State; msgA: Uint8Array } {
  const pw = passwordScalar(code);
  const X = Point.BASE.multiply(x).add(M.multiply(pw)).toBytes();
  return { state: { code, pw, x, X }, msgA: concat(Uint8Array.of(0x41), X) };
}

export function spake2Finish(state: Spake2State, msgB: Uint8Array): Uint8Array {
  if (msgB.length !== 33 || msgB[0] !== 0x42) throw new Error("Bad SPAKE2 message from host");
  const Ybytes = msgB.slice(1);
  let Y: EdPoint;
  try {
    Y = Point.fromBytes(Ybytes);
  } catch {
    throw new Error("Bad SPAKE2 message from host");
  }
  const K = Y.subtract(N.multiply(state.pw)).multiply(state.x).toBytes();
  return sha256(
    concat(
      sha256(utf8(state.code)),
      sha256(utf8(ID_A)),
      sha256(utf8(ID_B)),
      state.X,
      Ybytes,
      K,
    ),
  );
}

// ---------- §7 pairing proofs ----------

export const pairingProof = (sharedKey: Uint8Array, seenKeyHash: Uint8Array, publicKey: Uint8Array, deviceName: string) =>
  hmac(sha256, sharedKey, clientConfirm(seenKeyHash, publicKey, deviceName));

export function verifyHostProof(
  sharedKey: Uint8Array,
  seenKeyHash: Uint8Array,
  publicKey: Uint8Array,
  deviceId: string,
  hostProof: Uint8Array,
): boolean {
  const expected = hmac(sha256, sharedKey, hostConfirm(seenKeyHash, publicKey, deviceId));
  return constantTimeEqual(expected, hostProof);
}

/** "SHA256:" + first 8 bytes of SHA-256(public key), hex. Shown on host and phone. */
export const fingerprint = (publicKey: Uint8Array) => "SHA256:" + toHex(sha256(publicKey).slice(0, 8));

// ---------- device key and §8 login ----------

export function generateDeviceKey(): { seed: Uint8Array; publicKey: Uint8Array } {
  const seed = randomBytes(32);
  return { seed, publicKey: ed25519.getPublicKey(seed) };
}

export const publicKeyFromSeed = (seed: Uint8Array) => ed25519.getPublicKey(seed);

export const signLogin = (seed: Uint8Array, pinHash: Uint8Array, deviceId: string, nonce: Uint8Array) =>
  ed25519.sign(loginMessage(pinHash, deviceId, nonce), seed);
