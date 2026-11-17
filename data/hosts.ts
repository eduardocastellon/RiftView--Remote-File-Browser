// Paired hosts, stored on the phone (client_reqs.md §3).
//  - Host records (address, pin, device_id, names): app storage (SQLite key-value).
//  - Each host's Ed25519 private key seed: Keychain / Keystore, "this device
//    only", so it is never backed up or synced. One key pair per host.
// Nothing is written here until the host user approves the pairing (§12).

import * as SecureStore from "expo-secure-store";
import Storage from "expo-sqlite/kv-store";

export type HostRecord = {
  /** Our identity on this host, e.g. "dev_0011…". Also the record's key. */
  deviceId: string;
  /** IP or hostname the user entered. */
  address: string;
  port: number;
  /** base64 SHA-256 of the host certificate's public key (the pin). */
  pin: string;
  /** Name the host reports (GET /api/host), e.g. "Lalos-MacBook". */
  hostName: string | null;
  /** Name the user gave this host in the app, if any. */
  alias: string | null;
  pairedAtMs: number;
  lastConnectedAtMs: number | null;
};

const HOSTS_KEY = "riftview.hosts";
const seedKey = (deviceId: string) => `riftview.seed.${deviceId}`;
const SECURE_OPTIONS: SecureStore.SecureStoreOptions = {
  keychainAccessible: SecureStore.WHEN_UNLOCKED_THIS_DEVICE_ONLY,
};

/** Display name for lists: the user's alias, else the host's own name, else its address. */
export const hostDisplayName = (h: HostRecord) => h.alias || h.hostName || h.address;

export async function listHosts(): Promise<HostRecord[]> {
  const raw = await Storage.getItem(HOSTS_KEY);
  if (!raw) return [];
  try {
    return JSON.parse(raw) as HostRecord[];
  } catch {
    return [];
  }
}

export async function getHost(deviceId: string): Promise<HostRecord | null> {
  return (await listHosts()).find((h) => h.deviceId === deviceId) ?? null;
}

async function writeHosts(hosts: HostRecord[]): Promise<void> {
  await Storage.setItem(HOSTS_KEY, JSON.stringify(hosts));
}

/** Saves a newly approved host together with its private key seed. */
export async function saveHost(record: HostRecord, seedBase64: string): Promise<void> {
  await SecureStore.setItemAsync(seedKey(record.deviceId), seedBase64, SECURE_OPTIONS);
  const hosts = (await listHosts()).filter((h) => h.deviceId !== record.deviceId);
  await writeHosts([...hosts, record]);
}

export async function updateHost(deviceId: string, changes: Partial<Omit<HostRecord, "deviceId">>): Promise<void> {
  const hosts = await listHosts();
  await writeHosts(hosts.map((h) => (h.deviceId === deviceId ? { ...h, ...changes } : h)));
}

/** base64 Ed25519 seed for this host, or null if it's missing (e.g. app reinstalled). */
export function getSeed(deviceId: string): Promise<string | null> {
  return SecureStore.getItemAsync(seedKey(deviceId), SECURE_OPTIONS);
}

/** Forgets a host on this phone: deletes its record and its private key. */
export async function removeHost(deviceId: string): Promise<void> {
  await SecureStore.deleteItemAsync(seedKey(deviceId), SECURE_OPTIONS);
  await writeHosts((await listHosts()).filter((h) => h.deviceId !== deviceId));
}

export async function removeAllHosts(): Promise<void> {
  const hosts = await listHosts();
  await Promise.all(hosts.map((h) => SecureStore.deleteItemAsync(seedKey(h.deviceId), SECURE_OPTIONS)));
  await Storage.removeItem(HOSTS_KEY);
}
