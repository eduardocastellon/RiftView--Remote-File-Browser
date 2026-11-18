// Shares, folders and file details (client_reqs.md §9). All need `browse`.

import type { HostRecord } from "../../data/hosts";
import { authedRequest } from "./auth";
import { filePath, shareFilesPath, sharePath, sharesPath } from "./paths";

export type Share = { id: string; name: string; writable: boolean; created_at_ms: number };

export type FileEntry = {
  /** Opaque file ID: use it as-is for details and downloads, never parse it. */
  id: string;
  name: string;
  type: "directory" | "file";
  /** Path inside the share, starting with "/". */
  path: string;
  /** Files only. */
  size?: number;
  modified_ms?: number;
  /** Files only. */
  mime?: string;
};

export type FolderPage = {
  share_id: string;
  path: string;
  total: number;
  offset: number;
  limit: number;
  entries: FileEntry[];
};

export type SortKey = "name" | "size" | "modified";
export type SortOrder = "asc" | "desc";

export const PAGE_SIZE = 200;

export async function listShares(host: HostRecord): Promise<Share[]> {
  return (await authedRequest<{ shares: Share[] }>(host, "GET", sharesPath)).shares;
}

export const getShare = (host: HostRecord, shareId: string) =>
  authedRequest<Share>(host, "GET", sharePath(shareId));

/** One page of a folder. Folders always come before files. */
export function listFolder(
  host: HostRecord,
  shareId: string,
  opts: { path?: string; sort?: SortKey; order?: SortOrder; offset?: number; limit?: number } = {},
): Promise<FolderPage> {
  const query = [
    `path=${encodeURIComponent(opts.path ?? "/")}`,
    `sort=${opts.sort ?? "name"}`,
    `order=${opts.order ?? "asc"}`,
    `offset=${opts.offset ?? 0}`,
    `limit=${opts.limit ?? PAGE_SIZE}`,
  ].join("&");
  return authedRequest<FolderPage>(host, "GET", `${shareFilesPath(shareId)}?${query}`);
}

export const getFile = (host: HostRecord, fileId: string) =>
  authedRequest<FileEntry>(host, "GET", filePath(fileId));
