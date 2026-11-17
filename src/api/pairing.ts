// Pairing with a host, once per host (client_reqs.md §7).
//
//  1. probeHost()     – read the certificate key hash the host presents.
//  2. pairWithHost()  – SPAKE2 with the 6-digit code → HMAC proofs (which bind
//                       that certificate) → wait for the host user to approve
//                       → save → log in.
//
// Nothing is saved until the host says "approved". The code is never logged
// or stored (§12).

import * as Device from "expo-device";
import { Platform } from "react-native";
import { saveHost, updateHost, type HostRecord } from "../../data/hosts";
import { appVersion, login } from "./auth";
import { ApiError, ConnectionError, probePin, requestJson, type Connection } from "./client";
import { getHostInfo } from "./devices";
import { pairingCompletePath, pairingInitiatePath, pairingStatusPath } from "./paths";
import {
  fingerprint,
  fromBase64,
  generateDeviceKey,
  pairingProof,
  pinText,
  spake2Finish,
  spake2Start,
  toBase64,
  verifyHostProof,
} from "./protocol";

export type PairingFailure =
  | "no_pairing" // `pair` not running on the host, code expired, or claimed by another device
  | "wrong_code" // code burned: host must run `pair` again
  | "rejected" // host user said no
  | "expired" // 5-minute window passed
  | "cancelled" // pairing cancelled/replaced on host, or host restarted
  | "tampered" // proofs didn't verify: possible man in the middle
  | "rate_limited"
  | "unreachable"
  | "invalid" // bad input (name, version…), not burned
  | "aborted"; // user cancelled in the app

export class PairingError extends Error {
  constructor(
    readonly reason: PairingFailure,
    message: string,
  ) {
    super(message);
    this.name = "PairingError";
  }
}

export type PairingProgress =
  | { stage: "exchanging" }
  /** Show `fingerprint` so the user can compare it with the host terminal. */
  | { stage: "awaiting_approval"; fingerprint: string }
  | { stage: "logging_in" };

const POLL_INTERVAL_MS = 5_000; // faster can hit the rate limit (§10)
const RATE_LIMIT_BACKOFF_MS = 30_000;
const APPROVAL_DEADLINE_MS = 5.5 * 60_000; // code lives 5 minutes

/** Step 1: returns the base64 key hash of the certificate the host presents. */
export async function probeHost(address: string, port: number): Promise<string> {
  try {
    return await probePin(address, port);
  } catch (e) {
    throw new PairingError("unreachable", (e as Error).message);
  }
}

/** 1–64 chars after trimming, no control characters (§7.2). Trimmed BEFORE the HMAC. */
export function phoneName(): string {
  const raw = Device.deviceName || Device.modelName || "RiftView phone";
  const cleaned = raw.replace(/[\u0000-\u001f\u007f]/g, "").trim().slice(0, 64).trim();
  return cleaned || "RiftView phone";
}

function deviceType(): "phone" | "tablet" | "desktop" | "other" {
  switch (Device.deviceType) {
    case Device.DeviceType.PHONE:
      return "phone";
    case Device.DeviceType.TABLET:
      return "tablet";
    case Device.DeviceType.DESKTOP:
      return "desktop";
    default:
      return "other";
  }
}

const platform = (): "ios" | "android" | "other" =>
  Platform.OS === "ios" ? "ios" : Platform.OS === "android" ? "android" : "other";

function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(resolve, ms);
    signal?.addEventListener(
      "abort",
      () => {
        clearTimeout(timer);
        reject(new PairingError("aborted", "Pairing cancelled."));
      },
      { once: true },
    );
  });
}

function throwIfAborted(signal?: AbortSignal) {
  if (signal?.aborted) throw new PairingError("aborted", "Pairing cancelled.");
}

const RUN_PAIR_AGAIN = "Run `riftview-api pair` on the host to get a new code.";

function mapError(e: unknown, step: "initiate" | "complete" | "poll"): PairingError {
  if (e instanceof PairingError) return e;
  if (e instanceof ConnectionError) return new PairingError("unreachable", e.message);
  if (e instanceof ApiError) {
    if (e.status === 429) return new PairingError("rate_limited", "Too many attempts. Wait a minute and try again.");
    if (step === "initiate" && e.status === 409)
      return new PairingError("no_pairing", `The host isn't waiting for a phone, or the code expired. ${RUN_PAIR_AGAIN}`);
    if (step === "initiate" && e.status === 403)
      return new PairingError("cancelled", `The host rejected the request. ${RUN_PAIR_AGAIN}`);
    if (step === "complete" && e.status === 403)
      return new PairingError("wrong_code", `Wrong code. Codes only work once. ${RUN_PAIR_AGAIN}`);
    if (step === "complete" && (e.status === 400 || e.status === 422))
      return new PairingError("invalid", `The host refused this phone's details: ${e.message}`);
    if (e.status === 409) return new PairingError("cancelled", `Pairing was cancelled on the host. ${RUN_PAIR_AGAIN}`);
    return new PairingError("cancelled", e.message);
  }
  return new PairingError("cancelled", (e as Error)?.message ?? "Pairing failed.");
}

/**
 * Step 2. Resolves with the saved host once the host user approves it.
 * Throws PairingError otherwise.
 */
export async function pairWithHost(opts: {
  address: string;
  port: number;
  /** From probeHost(). */
  seenPin: string;
  /** Six digits; spaces are stripped. */
  code: string;
  /** Optional name the user gives this host in the app. */
  alias?: string;
  onProgress?: (p: PairingProgress) => void;
  signal?: AbortSignal;
}): Promise<HostRecord> {
  const { address, port, seenPin, signal, onProgress } = opts;
  const code = opts.code.replace(/\s/g, "");
  if (!/^\d{6}$/.test(code)) throw new PairingError("invalid", "Enter the 6-digit code shown on the host.");

  const seenKeyHash = fromBase64(seenPin);
  // All pairing requests are pinned to the certificate we probed. The proofs
  // below are what prove that certificate really belongs to the host.
  const conn: Connection = { address, port, pin: seenPin };
  const { seed, publicKey } = generateDeviceKey();
  const name = phoneName();

  onProgress?.({ stage: "exchanging" });

  // §7.1 key exchange
  const { state, msgA } = spake2Start(code);
  let pairingId: string;
  let sharedKey: Uint8Array;
  try {
    const { data } = await requestJson<{ pairing_id: string; spake2_message: string }>(conn, "POST", pairingInitiatePath, {
      body: { spake2_message: toBase64(msgA) },
    });
    pairingId = data.pairing_id;
    sharedKey = spake2Finish(state, fromBase64(data.spake2_message));
  } catch (e) {
    throw mapError(e, "initiate");
  }
  throwIfAborted(signal);

  // §7.2 prove the code, bind the certificate
  let completed: { stage: string; device_id: string; host_proof: string; host_pin: string };
  try {
    ({ data: completed } = await requestJson<typeof completed>(conn, "POST", pairingCompletePath, {
      body: {
        pairing_id: pairingId,
        device_name: name,
        device_public_key: toBase64(publicKey),
        device_type: deviceType(),
        platform: platform(),
        app_version: appVersion(),
        proof: toBase64(pairingProof(sharedKey, seenKeyHash, publicKey, name)),
      },
    }));
  } catch (e) {
    throw mapError(e, "complete");
  }

  // §7.3 verify the host (mandatory)
  const deviceId = completed.device_id;
  if (
    !verifyHostProof(sharedKey, seenKeyHash, publicKey, deviceId, fromBase64(completed.host_proof)) ||
    completed.host_pin !== pinText(seenKeyHash)
  ) {
    throw new PairingError(
      "tampered",
      "Couldn't verify the host. Something may be intercepting the connection. Nothing was saved.",
    );
  }
  throwIfAborted(signal);

  // §7.4 wait for the host user
  onProgress?.({ stage: "awaiting_approval", fingerprint: fingerprint(publicKey) });
  const deadline = Date.now() + APPROVAL_DEADLINE_MS;
  for (;;) {
    await sleep(POLL_INTERVAL_MS, signal);
    if (Date.now() > deadline) throw new PairingError("expired", `The code expired before it was approved. ${RUN_PAIR_AGAIN}`);
    let status: { stage: string; device_id?: string };
    try {
      ({ data: status } = await requestJson<typeof status>(conn, "GET", pairingStatusPath(pairingId)));
    } catch (e) {
      if (e instanceof ApiError && e.status === 429) {
        await sleep(RATE_LIMIT_BACKOFF_MS, signal);
        continue;
      }
      // A dropped request while waiting isn't fatal: keep polling until the deadline.
      if (e instanceof ConnectionError && !e.pinMismatch) continue;
      throw mapError(e, "poll");
    }
    if (status.stage === "awaiting_approval") continue;
    if (status.stage === "approved") break;
    if (status.stage === "rejected") throw new PairingError("rejected", "The host declined this phone.");
    if (status.stage === "expired") throw new PairingError("expired", `The code expired. ${RUN_PAIR_AGAIN}`);
    throw new PairingError("cancelled", `Pairing failed on the host. ${RUN_PAIR_AGAIN}`);
  }

  // Approved: now it's safe to save.
  const record: HostRecord = {
    deviceId,
    address,
    port,
    pin: seenPin,
    hostName: null,
    alias: opts.alias?.trim() || null,
    pairedAtMs: Date.now(),
    lastConnectedAtMs: null,
  };
  await saveHost(record, toBase64(seed));
  seed.fill(0);

  // §8 log in, and fetch the host's name for display. Pairing already
  // succeeded, so a failure here doesn't undo it.
  onProgress?.({ stage: "logging_in" });
  try {
    await login(record);
    const info = await getHostInfo(record);
    record.hostName = info.name;
    await updateHost(deviceId, { hostName: info.name });
  } catch {
    // Shown as offline/unnamed until the next successful connection.
  }
  return record;
}
