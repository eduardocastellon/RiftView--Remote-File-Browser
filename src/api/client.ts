// JSON requests to a host over the pinned native connection, with the
// server's error format (client_reqs.md §10) turned into ApiError.

import * as Tls from "../../modules/riftview-tls";

export type Connection = {
  address: string;
  port: number;
  /** base64 SHA-256 of the host's certificate public key. */
  pin: string;
};

/** The host answered with a non-2xx status. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    /** Server error code, e.g. "not_found"; null when the body wasn't JSON. */
    readonly code: string | null,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

/** Couldn't reach the host, or its certificate didn't match the pin. */
export class ConnectionError extends Error {
  constructor(
    message: string,
    readonly pinMismatch: boolean,
  ) {
    super(message);
    this.name = "ConnectionError";
  }
}

export const baseUrl = (c: Pick<Connection, "address" | "port">) => {
  // Bracket IPv6 literals.
  const host = c.address.includes(":") ? `[${c.address}]` : c.address;
  return `https://${host}:${c.port}`;
};

function toConnectionError(e: unknown): ConnectionError {
  const code = (e as { code?: string } | null)?.code;
  if (code === Tls.TlsErrorCode.PinMismatch) {
    return new ConnectionError("This host's identity changed since you paired. Pair with it again if you trust it.", true);
  }
  return new ConnectionError("Couldn't reach the host. Check the address and that RiftView is running.", false);
}

/** Probe the host's certificate before pairing (accepts any certificate). */
export async function probePin(address: string, port: number): Promise<string> {
  try {
    return await Tls.probePin(address, port);
  } catch (e) {
    throw toConnectionError(e);
  }
}

export async function requestJson<T>(
  conn: Connection,
  method: "GET" | "POST" | "DELETE",
  path: string,
  opts: { body?: unknown; token?: string } = {},
): Promise<{ status: number; data: T }> {
  const headers: Record<string, string> = { Accept: "application/json" };
  if (opts.body !== undefined) headers["Content-Type"] = "application/json";
  if (opts.token) headers.Authorization = `Bearer ${opts.token}`;

  let res: Tls.NativeResponse;
  try {
    res = await Tls.request({
      method,
      url: baseUrl(conn) + path,
      headers,
      body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
      pin: conn.pin,
    });
  } catch (e) {
    throw toConnectionError(e);
  }

  let parsed: unknown = undefined;
  if (res.body) {
    try {
      parsed = JSON.parse(res.body);
    } catch {
      // Plain-text body (JSON parse errors on the server side, §10).
    }
  }

  if (res.status < 200 || res.status >= 300) {
    const err = (parsed as { error?: { code?: string; message?: string } } | undefined)?.error;
    throw new ApiError(res.status, err?.code ?? null, err?.message ?? (res.body || `HTTP ${res.status}`));
  }
  return { status: res.status, data: parsed as T };
}
