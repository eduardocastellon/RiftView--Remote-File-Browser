// HTTPS to a RiftView host with a self-signed certificate (client_reqs.md §4).
// Stock fetch rejects self-signed certificates, so this goes through native code:
//  - probePin: connect accepting ANY certificate and report its key hash.
//    Used once, before pairing. Nothing is trusted yet; the pairing proofs
//    are what detect a man in the middle.
//  - request: accept the certificate ONLY if its key hash equals `pin`.
//    No system-trust fallback.
//
// Requires a development build (not available in Expo Go).

import { requireNativeModule } from "expo-modules-core";

export type NativeResponse = {
  status: number;
  /** Header names are lower-cased. */
  headers: Record<string, string>;
  body: string;
};

type RiftviewTlsNative = {
  probePin(host: string, port: number, timeoutMs: number): Promise<string>;
  request(
    method: string,
    url: string,
    headers: Record<string, string>,
    body: string | null,
    pin: string,
    timeoutMs: number,
  ): Promise<NativeResponse>;
};

const Native = requireNativeModule<RiftviewTlsNative>("RiftviewTls");

/** Error codes thrown by the native side (`error.code`). */
export const TlsErrorCode = {
  /** The certificate's key hash didn't match the pin: "host identity changed". */
  PinMismatch: "ERR_PIN_MISMATCH",
  /** Couldn't reach the host (offline, wrong address, timeout, TLS failure). */
  RequestFailed: "ERR_REQUEST_FAILED",
} as const;

/** Returns the base64 SHA-256 of the presented certificate's SubjectPublicKeyInfo. */
export function probePin(host: string, port: number, timeoutMs = 10_000): Promise<string> {
  return Native.probePin(host, port, timeoutMs);
}

export function request(opts: {
  method: string;
  url: string;
  headers?: Record<string, string>;
  body?: string;
  /** base64 SHA-256 of the expected SPKI (not the "sha256/" text). */
  pin: string;
  timeoutMs?: number;
}): Promise<NativeResponse> {
  return Native.request(
    opts.method,
    opts.url,
    opts.headers ?? {},
    opts.body ?? null,
    opts.pin,
    opts.timeoutMs ?? 30_000,
  );
}
