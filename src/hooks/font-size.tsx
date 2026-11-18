// App-wide text size preference (Settings → Text Size), saved in secure-store.
// Wrap the app in <FontSizeProvider>, then read the size with useFontSize().

import * as SecureStore from "expo-secure-store";
import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { font_size_levels, font_sizes, type FontSizeLevel } from "../../vars";

const STORAGE_KEY = "riftview.textSize";

type FontSizeContextValue = {
    level: FontSizeLevel;
    setLevel: (level: FontSizeLevel) => void;
};

const FontSizeContext = createContext<FontSizeContextValue>({
    level: "regular",
    setLevel: () => {},
});

const isLevel = (v: string | null): v is FontSizeLevel => font_size_levels.includes(v as FontSizeLevel);

export function FontSizeProvider({ children }: { children: ReactNode }) {
    const [level, setLevelState] = useState<FontSizeLevel>("regular");

    // Load the saved choice once at startup ("regular" until it arrives).
    useEffect(() => {
        SecureStore.getItemAsync(STORAGE_KEY)
            .then((saved) => {
                if (isLevel(saved)) setLevelState(saved);
            })
            .catch(() => {
                // Unreadable keychain: keep the default.
            });
    }, []);

    function setLevel(next: FontSizeLevel) {
        setLevelState(next);
        SecureStore.setItemAsync(STORAGE_KEY, next).catch(() => {
            // Couldn't save: the choice still applies until the app restarts.
        });
    }

    return <FontSizeContext.Provider value={{ level, setLevel }}>{children}</FontSizeContext.Provider>;
}

/** The current base text size in points. Re-renders when the setting changes. */
export function useFontSize(): number {
    return font_sizes[useContext(FontSizeContext).level];
}

/** The chosen level and a setter that also saves it. For the Settings screen. */
export function useFontSizeSetting(): FontSizeContextValue {
    return useContext(FontSizeContext);
}
