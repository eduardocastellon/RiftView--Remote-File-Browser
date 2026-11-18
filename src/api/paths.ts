// This file will store the paths the api fetches
// Any path the api fetches will be imported from here
// Example: export const pairPath: string = "/device/api"

export const DEFAULT_PORT = 7878;

// Public
export const healthPath = "/api/health";
export const versionPath = "/api/version";

// Pairing (client_reqs.md §7)
export const pairingInitiatePath = "/api/pairing/initiate";
export const pairingCompletePath = "/api/pairing/complete";
export const pairingStatusPath = (pairingId: string) => `/api/pairing/${encodeURIComponent(pairingId)}`;

// Login (§8)
export const authChallengePath = "/api/auth/challenge";
export const authSessionPath = "/api/auth/session";

// After login (§9)
export const devicesMePath = "/api/devices/me";
export const hostInfoPath = "/api/host";
export const sharesPath = "/api/shares";
export const sharePath = (shareId: string) => `${sharesPath}/${encodeURIComponent(shareId)}`;
export const shareFilesPath = (shareId: string) => `${sharePath(shareId)}/files`;
export const filePath = (fileId: string) => `/api/files/${encodeURIComponent(fileId)}`;
export const fileDownloadPath = (fileId: string) => `${filePath(fileId)}/download`;
