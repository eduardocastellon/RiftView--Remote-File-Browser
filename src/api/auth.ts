// Login on every connection (client_reqs.md §8): sign a single-use challenge
// with this host's device key and get a 1-hour bearer token.
// Tokens live only in memory and are never logged (§12).

import Constants from "expo-constants";
import { getSeed, updateHost, type HostRecord } from "../../data/hosts";
import { ApiError, requestJson, type Connection } from "./client";
import { authChallengePath, authSessionPath } from "./paths";
import { fromBase64, signLogin, toBase64 } from "./protocol";

/** The host no longer trusts this phone (revoked or unknown): offer to pair again. */
export class DeviceRemovedError extends Error {
  constructor(message = "This host no longer recognises this phone. Pair with it again.") {
    super(message);
    this.name = "DeviceRemovedError";
  }
}

type Session = { token: string; expiresAtMs: number };
const sessions = new Map<string, Session>();

export const connectionOf = (h: HostRecord): Connection => ({ address: h.address, port: h.port, pin: h.pin });

/** app_version allows 1–32 of letters, digits, "." "-" "+" (§7.2). */
export function appVersion(): string {
  const v = Constants.expoConfig?.version ?? "";
  return /^[A-Za-z0-9.+-]{1,32}$/.test(v) ? v : "1.0.0";
}

export async function login(host: HostRecord): Promise<string> {
  const seed = await getSeed(host.deviceId);
  if (!seed) throw new DeviceRemovedError("This phone's key for the host is missing. Pair with it again.");
  const conn = connectionOf(host);

  const { data: challenge } = await requestJson<{ challenge_id: string; nonce: string; expires_in_secs: number }>(
    conn,
    "POST",
    authChallengePath,
    { body: { device_id: host.deviceId } },
  );

  const signature = signLogin(fromBase64(seed), fromBase64(host.pin), host.deviceId, fromBase64(challenge.nonce));

  try {
    const { data } = await requestJson<{ token: string; expires_in_secs: number }>(conn, "POST", authSessionPath, {
      body: { challenge_id: challenge.challenge_id, signature: toBase64(signature), app_version: appVersion() },
    });
    // Refresh a minute early so a request never goes out with an expiring token.
    sessions.set(host.deviceId, {
      token: data.token,
      expiresAtMs: Date.now() + (data.expires_in_secs - 60) * 1000,
    });
    await updateHost(host.deviceId, { lastConnectedAtMs: Date.now() });
    return data.token;
  } catch (e) {
    // A fresh challenge with a correct signature still refused = removed from host.
    if (e instanceof ApiError && e.status === 401) throw new DeviceRemovedError();
    throw e;
  }
}

async function validToken(host: HostRecord): Promise<string> {
  const s = sessions.get(host.deviceId);
  if (s && s.expiresAtMs > Date.now()) return s.token;
  return login(host);
}

/** Authenticated request. On 401, logs in once more and retries (§8.3). */
export async function authedRequest<T>(
  host: HostRecord,
  method: "GET" | "POST" | "DELETE",
  path: string,
  body?: unknown,
): Promise<T> {
  const conn = connectionOf(host);
  try {
    return (await requestJson<T>(conn, method, path, { body, token: await validToken(host) })).data;
  } catch (e) {
    if (!(e instanceof ApiError && e.status === 401)) throw e;
    sessions.delete(host.deviceId);
    return (await requestJson<T>(conn, method, path, { body, token: await login(host) })).data;
  }
}

/** Ends the session on the host (best effort) and forgets the token. */
export async function logout(host: HostRecord): Promise<void> {
  const s = sessions.get(host.deviceId);
  sessions.delete(host.deviceId);
  if (!s) return;
  try {
    await requestJson(connectionOf(host), "DELETE", authSessionPath, { token: s.token });
  } catch {
    // Host offline or token already gone: nothing else to do.
  }
}
