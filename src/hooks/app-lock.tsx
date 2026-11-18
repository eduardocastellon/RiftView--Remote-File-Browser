// App lock (Settings → Require Face ID / Touch ID / passcode).
// When enabled, the app is locked at launch and whenever it goes to the
// background, and a lock screen covers everything until the user unlocks.
// The authentication itself lives in src/auth/local-auth.ts.

import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { AppState } from "react-native";
import { authenticate, getAuthMethod, isLockEnabled, saveLockEnabled, type AuthMethod } from "../auth/local-auth";
import LockScreen from "../components/lock-screen";

type AppLockContextValue = {
    /** What the phone can use to authenticate (refreshed when the app returns). */
    method: AuthMethod;
    enabled: boolean;
    /**
     * Turns the lock on or off. The user must authenticate first either way.
     * Resolves to an error message, or null on success.
     */
    setEnabled: (enabled: boolean) => Promise<string | null>;
};

const AppLockContext = createContext<AppLockContextValue>({
    method: { kind: "none", label: "" },
    enabled: false,
    setEnabled: async () => "App lock isn't ready yet.",
});

export function useAppLock(): AppLockContextValue {
    return useContext(AppLockContext);
}

export function AppLockProvider({ children }: { children: ReactNode }) {
    const [ready, setReady] = useState(false);
    const [enabled, setEnabledState] = useState(false);
    const [locked, setLockedState] = useState(false);
    const [method, setMethod] = useState<AuthMethod>({ kind: "none", label: "" });
    const [unlockError, setUnlockError] = useState<string | null>(null);

    // Only one system prompt at a time.
    const prompting = useRef(false);
    const enabledRef = useRef(false);
    // Mirrors `locked` for the AppState listener.
    const lockedRef = useRef(false);
    const setLocked = useCallback((value: boolean) => {
        lockedRef.current = value;
        setLockedState(value);
    }, []);

    const unlock = useCallback(async () => {
        if (prompting.current) return;
        prompting.current = true;
        setUnlockError(null);
        try {
            const result = await authenticate("Unlock RiftView");
            // If the phone's passcode/biometrics were removed, there is nothing
            // to check against; don't trap the user outside the app.
            if (result.ok || result.reason === "unavailable") {
                setLocked(false);
            } else if (result.reason !== "cancelled") {
                setUnlockError(result.message);
            }
        } finally {
            prompting.current = false;
        }
    }, [setLocked]);

    // Load the saved preference and the phone's capabilities at launch.
    useEffect(() => {
        Promise.all([isLockEnabled(), getAuthMethod()]).then(([on, m]) => {
            enabledRef.current = on;
            setEnabledState(on);
            setMethod(m);
            setLocked(on);
            setReady(true);
            if (on) unlock();
        });
    }, [unlock, setLocked]);

    // Lock when leaving the app; prompt again when coming back.
    // ("inactive" isn't used: the Face ID prompt itself makes the app inactive.)
    useEffect(() => {
        const sub = AppState.addEventListener("change", (state) => {
            if (state === "background" && enabledRef.current) {
                setLocked(true);
            } else if (state === "active") {
                getAuthMethod().then(setMethod); // the user may have changed phone settings
                if (lockedRef.current) unlock();
            }
        });
        return () => sub.remove();
    }, [unlock, setLocked]);

    const setEnabled = useCallback(async (next: boolean): Promise<string | null> => {
        if (prompting.current) return null;
        prompting.current = true;
        try {
            const result = await authenticate(next ? "Turn on app lock" : "Turn off app lock");
            if (!result.ok) return result.reason === "cancelled" ? null : result.message;
            await saveLockEnabled(next);
            enabledRef.current = next;
            setEnabledState(next);
            return null;
        } finally {
            prompting.current = false;
        }
    }, []);

    // Render nothing until we know whether to lock, so content never flashes.
    if (!ready) return null;

    return (
        <AppLockContext.Provider value={{ method, enabled, setEnabled }}>
            {children}
            <LockScreen visible={locked} methodLabel={method.label} error={unlockError} onUnlock={unlock} />
        </AppLockContext.Provider>
    );
}
