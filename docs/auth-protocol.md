# RiftView pairing and login protocol

How a client app (the React Native app) pairs with a RiftView host and logs in. The Rust reference client in [`tests/common/mod.rs`](../tests/common/mod.rs) (`device_pairing_attempt` and `log_in`) does exactly what this document describes; use it as an executable spec.

All binary values in JSON are **standard base64** (URL-safe base64 is also accepted).

## 1. Connection and certificate pinning

The host serves HTTPS on port `7878` on every interface (LAN, Tailscale, Radmin, Hamachi…). Its certificate is **self-signed**, so normal certificate checks fail. The client must:

1. Skip CA/hostname validation.
2. Compute the **pin** of the certificate presented: `SHA-256(SubjectPublicKeyInfo DER)`, i.e. the standard "public key pin" (the `sha256/…` format used by OkHttp `CertificatePinner` and curl `--pinnedpubkey`).
3. **Before pairing:** remember the 32-byte hash it saw and use it as `seen_key_hash` below. Do not trust the host yet.
4. **After pairing:** only connect if the presented key hash equals the stored pin. Refuse otherwise.

On iOS and Android this usually needs a small native piece (a `URLSessionDelegate` / custom `TrustManager`) that exposes the presented certificate's public key hash, because stock `fetch` rejects self-signed certificates.

The host user can see the pin with `riftview-api info`.

## 2. Byte formats

**Transcript**: several fields joined unambiguously. Each field is a 4-byte **big-endian** length followed by the field's bytes:

```
transcript(f1, f2, …) = len(f1) ‖ f1 ‖ len(f2) ‖ f2 ‖ …
```

| Name | Value |
|---|---|
| `client_confirm` | `transcript("riftview-pair-v1 client-confirm", seen_key_hash, device_public_key, utf8(device_name))` |
| `host_confirm` | `transcript("riftview-pair-v1 host-confirm", seen_key_hash, device_public_key, utf8(device_id))` |
| `login` | `transcript("riftview-login-v1", pinned_key_hash, utf8(device_id), nonce)` |

Labels are ASCII bytes. `seen_key_hash` / `pinned_key_hash` are the raw 32 bytes (not the `sha256/` text). `device_public_key` is the raw 32-byte Ed25519 public key.

## 3. Pairing (once per device)

The host user runs `riftview-api pair`, which shows a six-digit code valid for 5 minutes. The app asks the user for that code.

**Device key.** Generate an Ed25519 key pair and keep the private key in secure storage (Keychain / Android Keystore-backed storage). It never leaves the device.

**Step 1: SPAKE2.** The protocol is SPAKE2 over Ed25519 as implemented by the Rust [`spake2`](https://docs.rs/spake2/0.4.0) crate v0.4 (wire-compatible with `python-spake2` / Magic Wormhole).

- Password: the six digits as ASCII, e.g. `"482913"`.
- The device is side **A** with `idA = "riftview-client"`, `idB = "riftview-host"`.

```http
POST /api/pairing/initiate
{ "spake2_message": "<A's message>" }

200 { "pairing_id": "pair_…", "spake2_message": "<B's message>" }
```

Finish SPAKE2 with B's message to get the 32-byte `shared_key`. A wrong code is **not** detected yet; both sides just get different keys.

**Step 2: prove the code, bind the certificate.**

```http
POST /api/pairing/complete
{
  "pairing_id": "pair_…",
  "device_name": "Lalo's iPhone",            // 1–64 printable characters
  "device_public_key": "<32 bytes>",
  "proof": "<HMAC-SHA256(shared_key, client_confirm)>"
}

200 {
  "stage": "awaiting_approval",
  "device_id": "dev_…",
  "host_proof": "<HMAC-SHA256(shared_key, host_confirm)>",
  "host_pin": "sha256/…"
}
```

The device **must** recompute `host_confirm` with its own `seen_key_hash` and check `host_proof` (constant-time compare). Only if it matches:

- store `device_id`, and
- store `seen_key_hash` as the pin (it will equal `host_pin`).

If it doesn't match, abort: something is tampering with the connection.

A wrong proof returns `403` and **ends the pairing session**. The host must run `pair` again, so an attacker gets one guess per code.

**Step 3: wait for the host user.** The host terminal shows the device name and its key fingerprint (`SHA256:` + first 8 bytes of SHA-256 of the public key, in hex). The app can show the same fingerprint so the user can compare. Poll:

```http
GET /api/pairing/{pairing_id}

200 { "stage": "awaiting_approval" }
200 { "stage": "approved", "device_id": "dev_…" }
200 { "stage": "rejected" }          // also: "failed", "expired"
```

## 4. Login (every connection)

```http
POST /api/auth/challenge
{ "device_id": "dev_…" }

200 { "challenge_id": "…", "nonce": "<32 bytes>", "expires_in_secs": 60 }
```

Sign `login` (see §2) with the device's Ed25519 private key:

```http
POST /api/auth/session
{ "challenge_id": "…", "signature": "<64 bytes>" }

200 { "token": "…", "expires_in_secs": 3600 }
```

Each challenge works once and expires after 60 seconds. Send the token on every request:

```
Authorization: Bearer <token>
```

When a request returns `401`, log in again. `DELETE /api/auth/session` logs out.

`GET /api/devices/me` returns the device's ID, name and permissions (`browse`, `read`, `upload`, `rename`, `move`, `delete`). A missing permission gives `403`.

## 5. Errors and limits

Errors are JSON: `{"error": {"code": "unauthorized", "message": "…"}}`.

| Status | Meaning |
|---|---|
| 401 | Not logged in / token expired / login failed / device revoked |
| 403 | Missing permission, or pairing proof wrong |
| 409 | No pairing in progress, code expired, or another device is mid-pairing |
| 429 | Too many pairing/login requests from this IP (20 per minute by default) |

## 6. Client library notes

These are suggestions to evaluate, not tested dependencies:

- **Ed25519, SHA-256, HMAC:** `@noble/curves` and `@noble/hashes` (pure JS, audited).
- **Secure key storage:** `react-native-keychain` or `expo-secure-store`.
- **SPAKE2:** check any JS implementation for byte-exact compatibility with the Rust `spake2` 0.4 crate before relying on it. The alternatives are implementing it on `@noble/curves` following the crate's construction, or wrapping the Rust crate in a native module. Interop test vectors from the server side can be generated on request.
