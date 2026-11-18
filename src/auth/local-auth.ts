// Unlocking the app with the phone's own authentication: Face ID / Touch ID,
// fingerprint / face unlock on Android, or the device passcode as a fallback.
// Only the on/off preference is stored; biometric data never leaves the OS.

import * as LocalAuthentication from "expo-local-authentication";
import * as SecureStore from "expo-secure-store";
import { Platform } from "react-native";

const LOCK_KEY = "riftview.requireAuth";

export type AuthMethodKind = "face" | "fingerprint" | "iris" | "passcode" | "none";

export type AuthMethod = {
  kind: AuthMethodKind;
  /** User-facing name, e.g. "Face ID", "Touch ID", "fingerprint", "passcode". */
  label: string;
};

function labelFor(kind: AuthMethodKind): string {
  const ios = Platform.OS === "ios";
  switch (kind) {
    case "face": return ios ? "Face ID" : "face unlock";
    case "fingerprint": return ios ? "Touch ID" : "fingerprint";
    case "iris": return "iris unlock";
    case "passcode": return ios ? "passcode" : "screen lock";
    case "none": return "";
  }
}

/**
 * The best way this phone can authenticate the user right now.
 * Biometrics only count if they're set up; otherwise the device passcode
 * (PIN/pattern on Android) is used; "none" means the phone has no lock at all.
 */
export async function getAuthMethod(): Promise<AuthMethod> {
  const [hasHardware, enrolled, types, level] = await Promise.all([
    LocalAuthentication.hasHardwareAsync(),
    LocalAuthentication.isEnrolledAsync(),
    LocalAuthentication.supportedAuthenticationTypesAsync(),
    LocalAuthentication.getEnrolledLevelAsync(),
  ]);

  let kind: AuthMethodKind = "none";
  if (hasHardware && enrolled) {
    const { AuthenticationType } = LocalAuthentication;
    if (types.includes(AuthenticationType.FACIAL_RECOGNITION)) kind = "face";
    else if (types.includes(AuthenticationType.FINGERPRINT)) kind = "fingerprint";
    else if (types.includes(AuthenticationType.IRIS)) kind = "iris";
  }
  if (kind === "none" && level >= LocalAuthentication.SecurityLevel.SECRET) kind = "passcode";

  return { kind, label: labelFor(kind) };
}

export type AuthResult =
  | { ok: true }
  /**
   * `unavailable`: the phone has no biometrics or passcode set up any more,
   * so there is nothing to check against.
   */
  | { ok: false; reason: "cancelled" | "failed" | "locked_out" | "unavailable"; message: string };

/** Shows the system prompt (biometrics, falling back to the device passcode). */
export async function authenticate(promptMessage: string): Promise<AuthResult> {
  const result = await LocalAuthentication.authenticateAsync({
    promptMessage,
    cancelLabel: "Cancel",
    disableDeviceFallback: false,
  });
  if (result.success) return { ok: true };

  switch (result.error) {
    case "user_cancel":
    case "system_cancel":
    case "app_cancel":
      return { ok: false, reason: "cancelled", message: "Authentication was cancelled." };
    case "lockout":
      return { ok: false, reason: "locked_out", message: "Too many attempts. Unlock your phone with its passcode, then try again." };
    case "not_enrolled":
    case "passcode_not_set":
    case "not_available":
      return { ok: false, reason: "unavailable", message: "This phone has no Face ID, fingerprint or passcode set up." };
    default:
      return { ok: false, reason: "failed", message: "Couldn't verify it's you. Try again." };
  }
}

export async function isLockEnabled(): Promise<boolean> {
  try {
    return (await SecureStore.getItemAsync(LOCK_KEY)) === "1";
  } catch {
    return false;
  }
}

export async function saveLockEnabled(enabled: boolean): Promise<void> {
  if (enabled) await SecureStore.setItemAsync(LOCK_KEY, "1");
  else await SecureStore.deleteItemAsync(LOCK_KEY);
}
