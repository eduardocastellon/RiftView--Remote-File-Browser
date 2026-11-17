# RiftView mobile app: requirements for talking to the host

Everything the React Native app needs to **pair** with a RiftView host, **log in** on every connection, and **browse and download** files. It is written against the backend as it is now (phases 1–5, SQLite storage).

- The short protocol reference is [docs/auth-protocol.md](docs/auth-protocol.md). This document is the complete version, with app-side guidance and test vectors.
- The executable reference client is [tests/common/mod.rs](tests/common/mod.rs) (`device_pairing_attempt`, `log_in`). If this document and that code ever disagree, the code is right.

---

## 1. Overview

| | |
|---|---|
| Transport | HTTPS, self-signed certificate, **pinned** by public-key hash |
| Address | Host's IP on LAN / Tailscale / Radmin / Hamachi, port **7878** |
| Device identity ("passkey") | **Ed25519 key pair** generated on the phone; private key never leaves it |
| Pairing | Six-digit code shown on the host → **SPAKE2** key exchange → HMAC proofs → host user approves |
| Login | Every connection: sign a single-use challenge → bearer token valid **1 hour** |
| Data format | JSON; binary values are **standard base64** (with padding) |

"Passkey" here means **not WebAuthn**. It is a plain Ed25519 key pair kept in the phone's secure storage.

### Flow at a glance

```
Host terminal                      Phone app                           Server
─────────────                      ─────────                           ──────
riftview-api pair  ── shows code ─▶ user types 482913
                                   probe TLS, remember key hash
                                   SPAKE2 start (side A) ───────────▶ POST /api/pairing/initiate
                                   ◀──────────────── host SPAKE2 msg
                                   derive shared_key
                                   HMAC proof + device key ─────────▶ POST /api/pairing/complete
                                   ◀──────── host_proof, device_id, pin
                                   verify host_proof
"Lalo's iPhone wants to pair"      poll ────────────────────────────▶ GET /api/pairing/{id}
[r]ead-only [f]ull [n]o  ─────────▶ ◀──────────────────── "approved"
                                   save pin + device_id
                                   …later, every connection…
                                   ask challenge ───────────────────▶ POST /api/auth/challenge
                                   sign it ─────────────────────────▶ POST /api/auth/session
                                   ◀──────────────────────── token
                                   Authorization: Bearer <token> ───▶ /api/shares, /api/files/…
```

---

## 2. Technologies and libraries

The backend is Rust (Axum, rustls, `spake2` 0.4, `ed25519-dalek` 2, SQLite). The app needs these building blocks:

| Need | Suggested | Notes |
|---|---|---|
| Secure randomness | `react-native-get-random-values` | Polyfills `crypto.getRandomValues`, which the noble libraries need. Import it first in the app entry. |
| Ed25519 sign | `@noble/curves` (`ed25519`) | Pure JS, audited. RFC 8032 signatures, which the server checks with `verify_strict`. |
| Curve arithmetic for SPAKE2 | `@noble/curves` (ed25519 point API) | See §6. No ready-made JS SPAKE2 is known to be byte-compatible, so build it from these primitives. |
| SHA-256, HMAC-SHA256, HKDF-SHA256 | `@noble/hashes` | |
| Base64 | any (`base64-js`, Buffer polyfill) | Standard alphabet with `=` padding. |
| Secure key storage | `react-native-keychain` or `expo-secure-store` | Store the 32-byte Ed25519 seed. Use "this device only" / "when unlocked" accessibility so it isn't backed up or synced. |
| HTTPS with a self-signed cert + pinning | **Custom native module** (iOS + Android) | Stock `fetch` rejects self-signed certificates. See §4. |
| Downloads to disk with resume | The same native module, or `react-native-blob-util` routed through it | Needs `Range` support and the pinned trust. |

These libraries are suggestions and haven't been tested with this backend. The test vectors in §11 are how you prove your implementation is right.

---

## 3. What the app stores

**Per paired host** (one record per host; a phone can pair with several):

| Item | Where | Example |
|---|---|---|
| Ed25519 private key seed (32 bytes) | **Keychain / Keystore** | — |
| `device_id` | app storage | `dev_00112233445566778899aabbccddeeff` |
| Certificate pin (32 bytes, or the `sha256/…` text) | app storage | `sha256/DRyRmDU8liEI8uZU//uZ3izbDuUfmP8yrB7YSnP6kHw=` |
| Host address and port | app storage | `100.101.102.103:7878` |
| Display info (optional) | app storage | host name from `GET /api/host` |

Recommendation: use **one key pair per host**. The server doesn't require it, but it keeps hosts unlinkable and lets you forget one host without touching the others.

**Session token:** keep it in memory (or secure storage). It expires after 1 hour and is lost when the host restarts, so the app must expect to log in again often.

**What the host stores about the phone** (SQLite, `devices` and `device_permissions` tables; see [data_planning.md](data_planning.md)): id, name, custom_alias, public_key, key_version, device_type, platform, app_version, paired_at, last_connected, revoked_at and permissions. The app supplies `name`, `public_key`, `device_type`, `platform` and `app_version`. The host fills in the rest.

---

## 4. Connection and certificate pinning

The host serves HTTPS on `0.0.0.0:7878` (all interfaces). It uses a **self-signed ECDSA P-256 certificate** whose names are `riftview.local` and `localhost`. You'll connect by IP, so **hostname checks will always fail and must be skipped**. The pin replaces all CA and hostname validation.

### The pin

```
pin_hash = SHA-256( SubjectPublicKeyInfo DER of the server certificate )   // 32 bytes
pin_text = "sha256/" + base64(pin_hash)
```

This is the standard "public key pin" (the same format as OkHttp `CertificatePinner` and curl `--pinnedpubkey`).

- **Android:** `X509Certificate.getPublicKey().getEncoded()` is already the SPKI DER. Hash it.
- **iOS:** `SecCertificateCopyKey` + `SecKeyCopyExternalRepresentation` gives the **raw 65-byte EC point** (`04‖X‖Y`), not SPKI. Prepend the fixed P-256 SPKI header and then hash:

  ```
  3059301306072a8648ce3d020106082a8648ce3d030107034200   (26 bytes) ‖ 65-byte point
  ```

To check a pin from a computer:

```bash
openssl s_client -connect HOST:7878 </dev/null 2>/dev/null \
  | openssl x509 -pubkey -noout | openssl pkey -pubin -outform der \
  | openssl dgst -sha256 -binary | base64
```

The host user can also print it with `riftview-api info`.

### Native module behaviour

The module needs two modes:

1. **Probe (before pairing):** connect to `https://HOST:7878`, accept **any** certificate, and return the `pin_hash` of the certificate presented. The app keeps it as `seen_key_hash` and uses it during pairing. Do **not** trust this host yet. The pairing proofs (§6) detect a man in the middle.
2. **Pinned (after pairing):** for every request (and every new TLS connection), accept the certificate **only if** its `pin_hash` equals the stored pin. Otherwise fail the request and show a clear "host identity changed" error. Never fall back to the system trust store or offer "trust anyway".

Implementation hooks:

- **iOS:** a `URLSessionDelegate` handling `NSURLAuthenticationMethodServerTrust` in `urlSession(_:didReceive:completionHandler:)`.
- **Android:** a custom `X509TrustManager` (plus a `HostnameVerifier` that accepts the pinned host) on an `OkHttpClient`.

Expose a small API to JS, e.g.:

```ts
probePin(host: string, port: number): Promise<string /* base64 pin_hash */>
request(opts: { method, url, headers, body?, pin }): Promise<{ status, headers, body }>
download(opts: { url, headers, pin, destPath, rangeStart? }): Promise<{ status, bytesWritten }>
```

### Platform settings to check

- **iOS local network permission:** add `NSLocalNetworkUsageDescription` to Info.plist. iOS asks the user before the app can reach LAN IPs.
- **iOS App Transport Security:** connecting to raw IPs with a custom-trusted certificate may need ATS exceptions (e.g. `NSAllowsLocalNetworking`, or a broader exception for VPN ranges like Tailscale's `100.x`). Test on a real device over both LAN and VPN.
- **Android:** cleartext is never needed (everything is HTTPS). A custom TrustManager on your own `OkHttpClient` is enough; no `network_security_config` CA changes are required.

### Finding the host

There is no auto-discovery. The user enters the host's address:

- LAN: the computer's IP (e.g. `192.168.1.20`)
- Tailscale: `100.x.y.z`
- Radmin / Hamachi: the IP those apps show

Default port is `7878`. `riftview-api info` currently prints the bind address (`0.0.0.0:7878`) rather than the machine's real IPs, so the user has to look up their IP themselves.

---

## 5. Byte formats (shared by pairing and login)

### Transcript

Several fields joined so they can't be confused with one another. Each field is a **4-byte big-endian length** followed by the bytes:

```
transcript(f1, f2, …) = u32be(len(f1)) ‖ f1 ‖ u32be(len(f2)) ‖ f2 ‖ …
```

```ts
function transcript(...fields: Uint8Array[]): Uint8Array {
  const total = fields.reduce((sum, f) => sum + 4 + f.length, 0);
  const out = new Uint8Array(total);
  const view = new DataView(out.buffer);
  let offset = 0;
  for (const field of fields) {
    view.setUint32(offset, field.length, false); // big-endian
    out.set(field, offset + 4);
    offset += 4 + field.length;
  }
  return out;
}
```

### Messages

| Name | Value |
|---|---|
| `client_confirm` | `transcript("riftview-pair-v1 client-confirm", seen_key_hash, device_public_key, utf8(device_name))` |
| `host_confirm` | `transcript("riftview-pair-v1 host-confirm", seen_key_hash, device_public_key, utf8(device_id))` |
| `login` | `transcript("riftview-login-v1", pin_hash, utf8(device_id), nonce)` |

- Labels are ASCII bytes.
- `seen_key_hash` / `pin_hash` are the **raw 32 bytes**, not the `sha256/` text.
- `device_public_key` is the **raw 32-byte** Ed25519 public key.
- `device_name` must be the **trimmed** name (see the warning in §7.2).

### Constants

| Constant | Value |
|---|---|
| SPAKE2 identity A (the phone) | `"riftview-client"` |
| SPAKE2 identity B (the host) | `"riftview-host"` |
| SPAKE2 password | the six digits as ASCII, e.g. `"482913"` (no space) |

---

## 6. SPAKE2 (exactly as the Rust `spake2` 0.4 crate does it)

The server uses `Spake2::<Ed25519Group>` from the `spake2` 0.4 crate (wire-compatible with `python-spake2` / Magic Wormhole). **The phone is side A.** Every byte below has to match, or the keys differ and pairing fails with a burned code.

### Group and constants

- Curve: Ed25519 (twisted Edwards), base point `G` = the standard Ed25519 base point.
- Group order `L = 2^252 + 27742317777372353535851937790883648493`
  `= 7237005577332262213973186563042994240857116359379907606001950938285454250989`
- `M` (compressed point, hex): `15cfd18e385952982b6a8f8c7854963b58e34388c8e6dae891db756481a02312`
- `N` (compressed point, hex): `f04f2e7eb734b2a8f8b472eaf9c3c632576ac64aea650b496a8a20ff00e583c3`
- Points are encoded as the standard 32-byte compressed Ed25519 encoding.
- **No cofactor clearing** anywhere.

### Steps (side A)

1. **Password → scalar**
   ```
   okm = HKDF-SHA256(salt = "" (empty), ikm = ascii(code), info = "SPAKE2 pw", length = 48)
   pw  = int_from_BIG_endian(okm) mod L
   ```
   The crate reverses the 48 bytes into a little-endian buffer before reducing, which is the same as reading them big-endian.
2. **Random scalar:** `x` = uniformly random in `[1, L)`. Draw 64 random bytes and reduce mod `L`.
3. **Message A:** `X = x·G + pw·M`, then `msg_A = 0x41 ‖ compress(X)` (33 bytes; `0x41` = `'A'`).
4. Send `msg_A` (base64) to `POST /api/pairing/initiate` and receive `msg_B`.
5. **Check message B:** it must be 33 bytes, with `msg_B[0] == 0x42` (`'B'`) and `msg_B[1..]` decoding to a valid point `Y`. Otherwise abort.
6. **Shared point:** `K = x·(Y − pw·N)`.
7. **Key:**
   ```
   shared_key = SHA-256(
       SHA-256(ascii(code)) ‖ SHA-256("riftview-client") ‖ SHA-256("riftview-host")
       ‖ X (32 bytes) ‖ Y (32 bytes) ‖ compress(K) (32 bytes)
   )                                                                  // 32 bytes
   ```
   `X` and `Y` here are the 32-byte points **without** the side byte. Note that the first hash covers the raw code, not the scalar.

A wrong code is **not** detected by SPAKE2 itself; both sides just end up with different keys. The proof in §7.2 is what detects it.

### Sketch with noble

This is written against `@noble/curves` v1 names (`ed25519.ExtendedPoint`, `toRawBytes`). In v2 these are `ed25519.Point` and `toBytes`, and `@noble/hashes` import paths differ. Check your installed versions and validate against §11.

```ts
import 'react-native-get-random-values';
import { ed25519 } from '@noble/curves/ed25519';
import { sha256 } from '@noble/hashes/sha256';
import { hkdf } from '@noble/hashes/hkdf';

const Point = ed25519.ExtendedPoint;
const L = 2n ** 252n + 27742317777372353535851937790883648493n;
const M = Point.fromHex('15cfd18e385952982b6a8f8c7854963b58e34388c8e6dae891db756481a02312');
const N = Point.fromHex('f04f2e7eb734b2a8f8b472eaf9c3c632576ac64aea650b496a8a20ff00e583c3');
const ascii = (s: string) => new TextEncoder().encode(s);

const bytesToBigIntBE = (b: Uint8Array) =>
  b.reduce((acc, byte) => (acc << 8n) | BigInt(byte), 0n);

function passwordScalar(code: string): bigint {
  const okm = hkdf(sha256, ascii(code), new Uint8Array(0), ascii('SPAKE2 pw'), 48);
  return bytesToBigIntBE(okm) % L;
}

function randomScalar(): bigint {
  let x = 0n;
  while (x === 0n) x = bytesToBigIntBE(crypto.getRandomValues(new Uint8Array(64))) % L;
  return x;
}

export function spake2Start(code: string) {
  const pw = passwordScalar(code);
  const x = randomScalar();
  const X = Point.BASE.multiply(x).add(M.multiply(pw)).toRawBytes();
  return { state: { code, pw, x, X }, msgA: Uint8Array.of(0x41, ...X) };
}

export function spake2Finish(state, msgB: Uint8Array): Uint8Array {
  if (msgB.length !== 33 || msgB[0] !== 0x42) throw new Error('bad SPAKE2 message');
  const Ybytes = msgB.slice(1);
  const Y = Point.fromHex(Ybytes); // throws on an invalid point
  const K = Y.subtract(N.multiply(state.pw)).multiply(state.x).toRawBytes();
  return sha256(concat(
    sha256(ascii(state.code)), sha256(ascii('riftview-client')), sha256(ascii('riftview-host')),
    state.X, Ybytes, K,
  ));
}
```

(`concat` joins `Uint8Array`s. noble's `multiply` refuses a scalar of 0, which for `pw` is astronomically unlikely.)

---

## 7. Pairing (once per host)

**Before you start:** the host user runs `riftview-api pair` on the host computer. It prints a code like `482 913`, valid for **5 minutes**. Only one pairing can be in progress at a time. Running `pair` again cancels the previous one.

### 7.0 Prepare

1. Ask the user for the host address and the six digits (strip the space).
2. `probePin(host, 7878)` → `seen_key_hash` (32 bytes).
3. Generate a fresh Ed25519 key pair: 32 random bytes as the seed, `ed25519.getPublicKey(seed)` as the public key. Don't save it to the keychain until approval (or save it under a "pending" entry).

### 7.1 Key exchange

```http
POST /api/pairing/initiate
Content-Type: application/json

{ "spake2_message": "<base64 of msg_A (33 bytes)>" }
```

```http
200 OK
{ "pairing_id": "pair_…", "spake2_message": "<base64 of msg_B (33 bytes)>" }
```

Run `spake2Finish` to get `shared_key`.

### 7.2 Prove the code, bind the certificate

**⚠️ Trim the device name before using it anywhere.** The server trims leading and trailing whitespace before checking the proof. If you HMAC `"Phone "` but the server checks `"Phone"`, the proof fails and the code is **burned**.

Rules for `device_name`: 1–64 characters after trimming, no control characters (no newlines or tabs).

```
proof = HMAC-SHA256(key = shared_key, message = client_confirm)
        // client_confirm = transcript("riftview-pair-v1 client-confirm", seen_key_hash, public_key, utf8(name))
```

```http
POST /api/pairing/complete
Content-Type: application/json

{
  "pairing_id": "pair_…",
  "device_name": "Lalo's iPhone",
  "device_public_key": "<base64, 32 bytes>",
  "device_type": "phone",
  "platform": "ios",
  "app_version": "1.4.0",
  "proof": "<base64, 32 bytes>"
}
```

| Field | Allowed values |
|---|---|
| `device_type` | `phone`, `tablet`, `laptop`, `desktop`, `other` |
| `platform` | `ios`, `android`, `macos`, `windows`, `linux`, `other` |
| `app_version` | 1–32 characters: letters, digits, `.` `-` `+` (e.g. `1.4.0`, `1.4.0-beta.2+77`) |

The server rejects unknown JSON fields, so send exactly these.

```http
200 OK
{
  "stage": "awaiting_approval",
  "device_id": "dev_…",
  "host_proof": "<base64, 32 bytes>",
  "host_pin": "sha256/…"
}
```

### 7.3 Verify the host (mandatory)

```
expected = HMAC-SHA256(shared_key, host_confirm)
           // host_confirm = transcript("riftview-pair-v1 host-confirm", seen_key_hash, public_key, utf8(device_id))
```

- Compare `expected` with `host_proof` in **constant time**. If they differ, **abort**: something is intercepting the connection. Show an error and save nothing.
- `host_pin` should equal `"sha256/" + base64(seen_key_hash)`. If it doesn't, abort as well.
- If they match, keep `device_id` and `seen_key_hash` (the future pin) as **pending**.

Why this works: the code is never sent, and both proofs include the certificate key hash the phone actually saw. A man in the middle with its own certificate can't produce valid proofs without the code, and it gets only one guess.

### 7.4 Wait for the host user

The host terminal now shows the name, type, platform, app version and key fingerprint. Show the **same fingerprint** in the app so the user can compare:

```
fingerprint = "SHA256:" + hex( SHA-256(public_key)[0..8] )      // e.g. SHA256:6c8f8607dbe87077
```

Poll every **5 seconds** (faster polling can hit the rate limit, see §10):

```http
GET /api/pairing/{pairing_id}
```

| Response | Meaning | App action |
|---|---|---|
| `200 {"stage":"awaiting_approval"}` | Host hasn't answered | Keep polling |
| `200 {"stage":"approved","device_id":"dev_…"}` | Approved | Save the key seed (keychain), `device_id`, pin and address. Then log in (§8). |
| `200 {"stage":"rejected"}` | Host said no | Discard everything |
| `200 {"stage":"expired"}` | 5 minutes passed | Discard; ask the host to run `pair` again |
| `200 {"stage":"failed"}` | Bad proof earlier | Discard; host runs `pair` again |
| `409` | Pairing cancelled or replaced, or the server restarted | Discard; start over |

Approval must also happen within the code's 5-minute window.

### 7.5 Pairing errors

| Step | Status | Meaning |
|---|---|---|
| initiate | `409` | No pairing running (`pair` not started), code expired, or another device already claimed this code |
| initiate | `403` | Malformed SPAKE2 message (session is now failed) |
| complete | `403` | **Wrong code** or tampering. The session is burned and the host must run `pair` again. |
| complete | `400` | Invalid `device_name`, `app_version`, key or base64 (session **not** burned; fix and retry) |
| complete | `409` | Wrong `pairing_id`, expired, or wrong step |
| complete | `422` / `400` | JSON didn't parse, e.g. an unknown `platform` value or an extra field (session not burned) |
| any | `429` | Rate limited (§10) |

---

## 8. Login (every connection)

The token lives only in the host's memory. Log in when the app starts or reconnects, whenever a request returns `401`, and after the host restarts.

### 8.1 Get a challenge

```http
POST /api/auth/challenge
{ "device_id": "dev_…" }
```

```http
200 OK
{ "challenge_id": "…", "nonce": "<base64, 32 bytes>", "expires_in_secs": 60 }
```

The server issues a challenge for **any** `device_id`, even unknown ones, so this step never tells you whether you're still trusted. The next step does.

### 8.2 Sign and redeem

```
message   = transcript("riftview-login-v1", pin_hash, utf8(device_id), nonce)
signature = Ed25519.sign(message, seed)          // 64 bytes
```

```http
POST /api/auth/session
{
  "challenge_id": "…",
  "signature": "<base64, 64 bytes>",
  "app_version": "1.5.0"          // optional; send it so the host sees the current version
}
```

```http
200 OK
{ "token": "…", "expires_in_secs": 3600 }
```

- Each challenge works **once** and expires after **60 s**. Get a new one for every attempt.
- `401` means one of: the challenge was used or expired, the signature is wrong, or the **device was revoked** or unknown. If a fresh challenge plus a correct signature still gets `401`, treat the device as **removed from the host**, tell the user, and offer to pair again.
- `400` means an invalid `app_version`.
- A successful login records `last_connected` (and `app_version` if sent) on the host.

### 8.3 Using the token

```
Authorization: Bearer <token>
```

- Tokens are opaque strings; don't parse them.
- On any `401`, log in again once and retry the request. If it fails again, stop and show the error.
- Log out: `DELETE /api/auth/session` (with the token) → `204`.
- Revoking a device on the host ends its sessions **immediately**. Permission changes also apply on the very next request.

---

## 9. API after login

All of these need `Authorization: Bearer <token>`.

### Who am I

```http
GET /api/devices/me
200 {
  "id": "dev_…",
  "name": "Work phone",            // host's alias if set, else the name you paired with
  "device_type": "phone",
  "platform": "ios",
  "permissions": ["browse", "read"]
}
```

| Permission | Allows |
|---|---|
| `browse` | List shares and folders, see file details |
| `read` | Download files |
| `upload`, `rename`, `move`, `delete` | Reserved: the permissions exist, but the endpoints aren't built yet (phases 6–7). They will also require the share to be `writable`. |

A missing permission gives `403`. Use `permissions` to hide UI the device can't use.

### Host info

```http
GET /api/host
200 { "name": "Lalos-MacBook", "os": "macos", "arch": "aarch64", "version": "0.1.0" }
```

### Shares (needs `browse`)

```http
GET /api/shares
200 { "shares": [ { "id": "shr_…", "name": "Photos", "writable": false, "created_at_ms": 1794800000000 } ] }

GET /api/shares/{share_id}
200 { "id": "shr_…", "name": "Photos", "writable": false, "created_at_ms": 1794800000000 }
```

### Browse a folder (needs `browse`)

```http
GET /api/shares/{share_id}/files?path=/Documents&sort=name&order=asc&offset=0&limit=200
```

| Query | Default | Values |
|---|---|---|
| `path` | `/` | Path inside the share, starting with `/`. URL-encode it. `..`, backslashes and NUL are rejected (`400`). |
| `sort` | `name` | `name`, `size`, `modified` |
| `order` | `asc` | `asc`, `desc` |
| `offset` | `0` | Paging |
| `limit` | `200` | 1–1000 |

```json
{
  "share_id": "shr_…",
  "path": "/Documents",
  "total": 3,
  "offset": 0,
  "limit": 200,
  "entries": [
    { "id": "…", "name": "Projects", "type": "directory", "path": "/Documents/Projects", "modified_ms": 1794800000000 },
    { "id": "…", "name": "notes.txt", "type": "file", "path": "/Documents/notes.txt",
      "size": 11, "modified_ms": 1794800000000, "mime": "text/plain" }
  ]
}
```

- Folders always come before files.
- `size` and `mime` appear only for files, and `modified_ms` may be missing. Treat all three as optional.
- `id` is an **opaque file ID**. Use it for metadata and downloads, and don't parse or build it yourself.
- A folder with more than 100,000 entries returns `409`.

### File details (needs `browse`)

```http
GET /api/files/{file_id}
200 { "id": "…", "name": "notes.txt", "type": "file", "path": "/Documents/notes.txt", "size": 11, "modified_ms": …, "mime": "text/plain" }
```

### Download (needs `read`)

```http
GET /api/files/{file_id}/download
Range: bytes=1048576-          // optional, for resuming
```

| Status | Meaning |
|---|---|
| `200` | Whole file |
| `206` | Partial; `Content-Range: bytes START-END/TOTAL` |
| `416` | Range past the end; `Content-Range: bytes */TOTAL` tells you the real size |

- Response headers: `Content-Type`, `Content-Length`, `Accept-Ranges: bytes`, `Content-Disposition` (file name), `Cache-Control: no-store`.
- Only **one** range per request (`bytes=A-B`, `bytes=A-` or `bytes=-N`).
- To resume: keep the partial file, then request `Range: bytes=<bytes already written>-`.
- The file streams in chunks. Write it straight to disk instead of buffering it in JS.
- **Not available yet:** a SHA-256 checksum to verify downloads. Compare the final size with `size` for now.

---

## 10. Errors, limits and timeouts

Errors from the server are JSON:

```json
{ "error": { "code": "not_found", "message": "not found" } }
```

| Status | `code` | Typical cause |
|---|---|---|
| 400 | `bad_request` | Invalid input (path, name, version, base64) |
| 401 | `unauthorized` | No or expired token, failed login, revoked device. Has `WWW-Authenticate: Bearer`. |
| 403 | `forbidden` | Missing permission, or wrong pairing proof |
| 404 | `not_found` | Unknown share, file or route |
| 409 | `conflict` | Pairing state problems, folder too large |
| 416 | `range_not_satisfiable` | Bad `Range` |
| 429 | `too_many_requests` | Rate limit |
| 503 | `unavailable` | Share folder is missing on the host (e.g. unplugged drive) |
| 500 | `internal` | Host-side problem (details only in the host log) |

The exception is when the JSON body itself doesn't parse (bad JSON, wrong type, unknown field, unknown enum value): the server returns `400`/`415`/`422` with a **plain-text** body. Always send `Content-Type: application/json`.

| Limit | Value |
|---|---|
| Rate limit (pairing + login routes **combined**, per IP) | **20 requests / minute** |
| Pairing code lifetime | 5 minutes |
| Login challenge lifetime | 60 seconds |
| Session token lifetime | 1 hour |
| Request body size | 64 KiB |
| Time for the server to start responding | 30 seconds |

Rate-limit budget: pairing uses 2 requests plus polls, and each login uses 2. Polling every 5 s stays well under the limit. On `429`, back off for about 30–60 s.

`GET /api/health` (`{"status":"ok"}`) and `GET /api/version` (`{"name":"riftview-api","version":"0.1.0"}`) are public. Use them for "is the host reachable" checks (with the pinned connection).

---

## 11. Test vectors

These were generated from the real Rust crates (`spake2` 0.4 with fixed scalars, and the server's own protocol code). Your implementation must reproduce every line. Hex unless noted.

### SPAKE2

| | |
|---|---|
| code | `482913` |
| HKDF okm (48 bytes) | `97df4716d3f314f9933bfdff6f2066e533be05276600edfb478fa3adea4dbd76d1b7dbbee9bfa83d81046591589044f9` |
| `pw` scalar (32 bytes, **little-endian**) | `2b7cba4a934f417a16e7808211ae7b66974940faf49ba56f78a023798c6ea20d` |
| `x` (side A scalar, little-endian) | `9ee347fe6465c8674f6114ebf4302f82b10cbb23307fcb0df3b47a3b6815e80a` |
| `y` (side B scalar, little-endian) | `4ff3999faf677e77c82531330b687fef6219764760fe961be669f576d02ad005` |
| `msg_A` (33 bytes) | `4168089307f4e84913874404904b1185e23944847f5e0dae2a41d8cf79e7c1a197` |
| `msg_A` base64 | `QWgIkwf06EkTh0QEkEsRheI5RIR/Xg2uKkHYz3nnwaGX` |
| `msg_B` (33 bytes) | `426c36bf8dba0759a49664b6edafcda1cac1fb23c8acefc70dd5417039672cdb91` |
| `shared_key` | `f7f0b803a8a5cba5f8822e1cfbd5ee01c84ea7087fa0d551addcc783dd3f3a71` |

To test: set `x` to the value above instead of a random one, check that `msg_A` matches, then feed `msg_B` in and check `shared_key`.

### Pairing proofs

| | |
|---|---|
| `seen_key_hash` | `0d1c9198353c962108f2e654fffb99de2cdb0ee51f98ff32ac1ed84a73fa907c` |
| pin text | `sha256/DRyRmDU8liEI8uZU//uZ3izbDuUfmP8yrB7YSnP6kHw=` |
| device seed (Ed25519 private) | `3333333333333333333333333333333333333333333333333333333333333333` |
| device public key | `17cb79fb2b4120f2b1ec65e4198d6e08b28e813feb01e4a400839b85e18080ce` |
| device public key base64 | `F8t5+ytBIPKx7GXkGY1uCLKOgT/rAeSkAIObheGAgM4=` |
| fingerprint | `SHA256:6c8f8607dbe87077` |
| device_name | `Lalo's iPhone` |
| `client_confirm` | `0000001f72696674766965772d706169722d763120636c69656e742d636f6e6669726d000000200d1c9198353c962108f2e654fffb99de2cdb0ee51f98ff32ac1ed84a73fa907c0000002017cb79fb2b4120f2b1ec65e4198d6e08b28e813feb01e4a400839b85e18080ce0000000d4c616c6f2773206950686f6e65` |
| `proof` | `78f19823a8df2412c93771dccfe228fb46edb8755a80a7e597c33af5c8559a46` |
| `proof` base64 | `ePGYI6jfJBLJN3Hcz+Io+0btuHVagKfll8M69chVmkY=` |
| device_id | `dev_00112233445566778899aabbccddeeff` |
| `host_confirm` | `0000001d72696674766965772d706169722d763120686f73742d636f6e6669726d000000200d1c9198353c962108f2e654fffb99de2cdb0ee51f98ff32ac1ed84a73fa907c0000002017cb79fb2b4120f2b1ec65e4198d6e08b28e813feb01e4a400839b85e18080ce000000246465765f3030313132323333343435353636373738383939616162626363646465656666` |
| `host_proof` | `73afa144ffe02d631aa090839674faf43e2253b1d2da678fdec1788579d053b8` |

### Login

| | |
|---|---|
| nonce | `4444444444444444444444444444444444444444444444444444444444444444` |
| `login` message | `0000001172696674766965772d6c6f67696e2d7631000000200d1c9198353c962108f2e654fffb99de2cdb0ee51f98ff32ac1ed84a73fa907c000000246465765f3030313132323333343435353636373738383939616162626363646465656666000000204444444444444444444444444444444444444444444444444444444444444444` |
| signature | `9116f2aeffd006c7c078b9d3e7d10c8a59a4595b3052a242d1c93f07b4dc01895895721be0c1bcb97bed528d928004e801c6b90f8e6dabdd904f4bae873de908` |
| signature base64 | `kRbyrv/QBsfAeLnT59EMilmkWVswUqJC0ck/B7TcAYlYlXIb4MG8uXvtUo2SgAToAca5D45tq92QT0uuhz3pCA==` |

Ed25519 signatures are deterministic, so your signature must match exactly.

---

## 12. Security checklist for the app

- [ ] Private key seed only in Keychain / Keystore, "this device only", never logged, never sent.
- [ ] Pairing aborts if `host_proof` doesn't verify or `host_pin` doesn't match `seen_key_hash`.
- [ ] Proofs and HMACs compared in constant time.
- [ ] After pairing, every connection is pinned; no system-trust fallback and no "trust anyway" button.
- [ ] Device name trimmed before it's sent and before the HMAC.
- [ ] The pairing code is never logged or stored.
- [ ] Nothing is saved as trusted until the host says `approved`.
- [ ] Persistent `401` after a fresh login means "removed from host", so offer to pair again.
- [ ] Token kept out of logs and crash reports.
- [ ] Downloads streamed to disk with resume; size checked at the end.

---

## 13. Not built yet on the backend

| Feature | Status |
|---|---|
| Uploads, rename, move, delete | Phases 6–7. Permissions already exist. |
| Download checksums (SHA-256) | Planned for Phase 5 |
| Host auto-discovery (mDNS) | Not planned yet; manual address entry |
| Device key rotation | `key_version` is stored (always 1); no endpoint yet |
| Server-side app version gating | `app_version` is recorded but not enforced |
