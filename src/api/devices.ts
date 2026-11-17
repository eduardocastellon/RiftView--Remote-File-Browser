// code for adding, deleting, handling devices
// (Adding a host is pairing: see ./pairing.ts.)

import { getHost, listHosts, removeAllHosts, removeHost, type HostRecord } from "../../data/hosts";
import { authedRequest, logout } from "./auth";
import { devicesMePath, hostInfoPath } from "./paths";

export type HostInfo = { name: string; os: string; arch: string; version: string };

export type Permission = "browse" | "read" | "upload" | "rename" | "move" | "delete";

export type DeviceMe = {
  id: string;
  /** The host's alias for this phone if set, else the name it paired with. */
  name: string;
  device_type: string;
  platform: string;
  permissions: Permission[];
};

export { getHost, listHosts };

export const getHostInfo = (host: HostRecord) => authedRequest<HostInfo>(host, "GET", hostInfoPath);

export const getMe = (host: HostRecord) => authedRequest<DeviceMe>(host, "GET", devicesMePath);

/**
 * Forgets a host on this phone: ends the session and deletes the saved key.
 * The host keeps its record until its user revokes the device there.
 */
export async function forgetHost(host: HostRecord): Promise<void> {
  await logout(host);
  await removeHost(host.deviceId);
}

export async function forgetAllHosts(): Promise<void> {
  const hosts = await listHosts();
  await Promise.all(hosts.map(logout));
  await removeAllHosts();
}
