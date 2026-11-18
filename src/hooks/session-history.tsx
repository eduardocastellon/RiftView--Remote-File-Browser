// Back / forward history for a browsing session (like a file browser).
//
// Every folder screen calls `visit(location)` when it gains focus. Comparing
// that with the history tells us what happened, however the user navigated:
//   - same as the current entry      → nothing (e.g. a sheet closed)
//   - same as an earlier entry       → went back (button, swipe or Home)
//   - same as the next entry         → went forward
//   - anything else                  → opened a new folder: drop forward history
//
// Back pops the screen (so swipe-back keeps working); Forward pushes the next
// entry again. Lives in session/_layout.tsx, so it resets when the session ends.

import { router } from "expo-router";
import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";

export type SessionLocation = {
    /** Missing = the host's share list. */
    share?: string;
    /** Folder inside the share. */
    path?: string;
    /** Header title (share or folder name). */
    title?: string;
};

type History = { entries: SessionLocation[]; index: number };

type SessionHistoryValue = {
    current: SessionLocation | undefined;
    canGoBack: boolean;
    canGoForward: boolean;
    goBack: () => void;
    goForward: () => void;
    /** Back to the share list. */
    goHome: () => void;
    /** Leave the session and return to the device list. */
    exit: () => void;
    /** Called by each folder screen on focus. */
    visit: (location: SessionLocation) => void;

    /** Search bar in the bottom bar: filters the current folder by name. */
    searchOpen: boolean;
    searchQuery: string;
    openSearch: () => void;
    setSearchQuery: (query: string) => void;
    closeSearch: () => void;
};

const SessionHistoryContext = createContext<SessionHistoryValue | null>(null);

export function useSessionHistory(): SessionHistoryValue {
    const value = useContext(SessionHistoryContext);
    if (!value) throw new Error("useSessionHistory must be used inside a session (SessionHistoryProvider).");
    return value;
}

const sameLocation = (a: SessionLocation | undefined, b: SessionLocation | undefined) =>
    !!a && !!b && (a.share ?? "") === (b.share ?? "") && (a.share ? (a.path ?? "/") === (b.path ?? "/") : true);

/** Identifies a location, so search state can belong to one folder. */
const locationKey = (loc: SessionLocation | undefined) => (loc ? `${loc.share ?? ""}|${loc.share ? (loc.path ?? "/") : ""}` : "");

/** Applies a visit to the history (pure, so it's easy to reason about). */
function applyVisit(h: History, loc: SessionLocation): History {
    if (h.entries.length === 0) return { entries: [loc], index: 0 };
    if (sameLocation(loc, h.entries[h.index])) return h;
    if (sameLocation(loc, h.entries[h.index + 1])) return { ...h, index: h.index + 1 };
    for (let k = h.index - 1; k >= 0; k--) {
        if (sameLocation(loc, h.entries[k])) return { ...h, index: k };
    }
    const entries = [...h.entries.slice(0, h.index + 1), loc];
    return { entries, index: entries.length - 1 };
}

export function SessionHistoryProvider({ deviceId, children }: { deviceId: string; children: ReactNode }) {
    const [history, setHistory] = useState<History>({ entries: [], index: 0 });
    // Search belongs to the folder it was opened in: navigating anywhere else
    // closes and clears it, without needing an effect to reset it.
    const [search, setSearch] = useState<{ key: string; query: string } | null>(null);
    const currentKey = locationKey(history.entries[history.index]);
    const searchOpen = search !== null && search.key === currentKey;
    const searchQuery = searchOpen ? search.query : "";

    const openSearch = useCallback(() => setSearch({ key: currentKey, query: "" }), [currentKey]);
    const setSearchQuery = useCallback((query: string) => setSearch({ key: currentKey, query }), [currentKey]);
    const closeSearch = useCallback(() => setSearch(null), []);

    const visit = useCallback((loc: SessionLocation) => {
        setHistory((h) => applyVisit(h, loc));
    }, []);

    const goBack = useCallback(() => {
        if (history.index > 0 && router.canGoBack()) router.back();
    }, [history.index]);

    const goForward = useCallback(() => {
        const next = history.entries[history.index + 1];
        if (!next) return;
        router.push({
            pathname: "/session",
            params: { id: deviceId, share: next.share, path: next.path, title: next.title },
        });
    }, [deviceId, history]);

    const goHome = useCallback(() => {
        // Pops back to the first screen of the session's stack (the share list).
        if (history.index > 0) router.dismissAll();
    }, [history.index]);

    const exit = useCallback(() => {
        router.dismissTo("/");
    }, []);

    const value = useMemo<SessionHistoryValue>(
        () => ({
            current: history.entries[history.index],
            canGoBack: history.index > 0,
            canGoForward: history.index < history.entries.length - 1,
            goBack,
            goForward,
            goHome,
            exit,
            visit,
            searchOpen,
            searchQuery,
            openSearch,
            setSearchQuery,
            closeSearch,
        }),
        [history, goBack, goForward, goHome, exit, visit, searchOpen, searchQuery, openSearch, setSearchQuery, closeSearch],
    );

    return <SessionHistoryContext.Provider value={value}>{children}</SessionHistoryContext.Provider>;
}
