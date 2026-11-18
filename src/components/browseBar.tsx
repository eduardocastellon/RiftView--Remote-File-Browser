// Floating bar at the bottom of a browsing session. Drawn once by
// session/_layout.tsx, so it stays put while folders slide in and out under it.
//
//   back · forward · search · menu
//
// - search: the bar morphs into a search field that filters the current
//   folder by name (the folder screen reads the query from the session).
// - menu:   opens a small panel above the bar with more options (Home, Exit).

import { useEffect, useState, type ReactNode } from "react";
import {
    Keyboard,
    LayoutAnimation,
    Platform,
    Pressable,
    StyleSheet,
    Text,
    TextInput,
    TouchableOpacity,
    View,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { useFontSize } from "../hooks/font-size";
import { useSessionHistory } from "../hooks/session-history";
import { ChevronLeftIcon, ChevronRightIcon, HouseIcon, LogOutIcon, MenuIcon, SearchIcon, XIcon } from "./icons";

export const BROWSE_BAR_HEIGHT = 56;
/** Gap between the bar and the bottom safe-area edge (or the keyboard). */
const BAR_MARGIN = 12;

const ACTIVE = "#007AFF";
const DISABLED = "#C7C7CC";
const DESTRUCTIVE = "#D70015";

/** Bottom padding a scrolling list needs so its last rows can scroll above the bar. */
export function useBrowseBarInset(): number {
    const insets = useSafeAreaInsets();
    return BROWSE_BAR_HEIGHT + BAR_MARGIN + insets.bottom + 12;
}

/** Height of the on-screen keyboard (0 when hidden), so the bar can sit above it. */
function useKeyboardHeight(): number {
    const [height, setHeight] = useState(0);
    useEffect(() => {
        // iOS has "will" events, so the bar moves together with the keyboard.
        const showEvent = Platform.OS === "ios" ? "keyboardWillShow" : "keyboardDidShow";
        const hideEvent = Platform.OS === "ios" ? "keyboardWillHide" : "keyboardDidHide";
        const show = Keyboard.addListener(showEvent, (e) => {
            LayoutAnimation.configureNext(LayoutAnimation.create(e.duration || 250, "keyboard", "opacity"));
            setHeight(e.endCoordinates.height);
        });
        const hide = Keyboard.addListener(hideEvent, (e) => {
            LayoutAnimation.configureNext(LayoutAnimation.create(e.duration || 250, "keyboard", "opacity"));
            setHeight(0);
        });
        return () => {
            show.remove();
            hide.remove();
        };
    }, []);
    return height;
}

/** Animates the next layout change (bar ⇄ search field, menu open/close). */
const animateLayout = () => LayoutAnimation.configureNext(LayoutAnimation.Presets.easeInEaseOut);

function IconButton({ label, onPress, disabled, children }: {
    label: string;
    onPress: () => void;
    disabled?: boolean;
    children: ReactNode;
}) {
    return (
        <TouchableOpacity
            style={styles.iconButton}
            onPress={onPress}
            disabled={disabled}
            hitSlop={8}
            accessibilityRole="button"
            accessibilityLabel={label}
            accessibilityState={{ disabled }}
        >
            {children}
        </TouchableOpacity>
    );
}

function MenuItem({ label, icon, onPress, disabled, destructive }: {
    label: string;
    icon: ReactNode;
    onPress: () => void;
    disabled?: boolean;
    destructive?: boolean;
}) {
    const font_size = useFontSize();
    const color = disabled ? DISABLED : destructive ? DESTRUCTIVE : "#000";
    return (
        <TouchableOpacity
            style={styles.menuItem}
            onPress={onPress}
            disabled={disabled}
            accessibilityRole="button"
            accessibilityState={{ disabled }}
        >
            {icon}
            <Text style={{ fontSize: font_size, color }}>{label}</Text>
        </TouchableOpacity>
    );
}

export default function BrowseBar() {
    const insets = useSafeAreaInsets();
    const keyboardHeight = useKeyboardHeight();
    const font_size = useFontSize();
    const iconSize = font_size + 8; // 24 at the regular text size
    const {
        canGoBack, canGoForward, goBack, goForward, goHome, exit,
        searchOpen, searchQuery, openSearch, setSearchQuery, closeSearch,
    } = useSessionHistory();
    const [menuOpen, setMenuOpen] = useState(false);

    // Above the keyboard while typing, else above the home indicator.
    const barBottom = (keyboardHeight > 0 ? keyboardHeight : insets.bottom) + BAR_MARGIN;

    function startSearch() {
        animateLayout();
        setMenuOpen(false);
        openSearch();
    }

    function endSearch() {
        animateLayout();
        Keyboard.dismiss();
        closeSearch();
    }

    function toggleMenu() {
        animateLayout();
        setMenuOpen((open) => !open);
    }

    function fromMenu(action: () => void) {
        return () => {
            setMenuOpen(false);
            action();
        };
    }

    return (
        <>
            {menuOpen && (
                // Tap anywhere outside the menu to close it.
                <Pressable style={StyleSheet.absoluteFill} onPress={toggleMenu} accessibilityLabel="Close menu" />
            )}

            {menuOpen && (
                <View style={[styles.menu, { bottom: barBottom + BROWSE_BAR_HEIGHT + 8 }]}>
                    <MenuItem
                        label="Home"
                        icon={<HouseIcon size={iconSize - 4} color={canGoBack ? "#000" : DISABLED} />}
                        onPress={fromMenu(goHome)}
                        disabled={!canGoBack}
                    />
                    {/* More options go here. */}
                    <View style={styles.menuDivider} />
                    <MenuItem
                        label="Exit session"
                        icon={<LogOutIcon size={iconSize - 4} color={DESTRUCTIVE} />}
                        onPress={fromMenu(exit)}
                        destructive
                    />
                </View>
            )}

            <View style={[styles.bar, { bottom: barBottom }]}>
                {searchOpen ? (
                    <>
                        <SearchIcon size={iconSize - 4} color="#8E8E93" />
                        <TextInput
                            style={[styles.searchInput, { fontSize: font_size }]}
                            value={searchQuery}
                            onChangeText={setSearchQuery}
                            placeholder="Search this folder"
                            placeholderTextColor="#8E8E93"
                            autoFocus
                            autoCapitalize="none"
                            autoCorrect={false}
                            returnKeyType="search"
                            onSubmitEditing={() => Keyboard.dismiss()}
                            accessibilityLabel="Search this folder"
                        />
                        <IconButton label="Close search" onPress={endSearch}>
                            <XIcon size={iconSize} color={ACTIVE} />
                        </IconButton>
                    </>
                ) : (
                    <>
                        <IconButton label="Back" onPress={goBack} disabled={!canGoBack}>
                            <ChevronLeftIcon size={iconSize} color={canGoBack ? ACTIVE : DISABLED} />
                        </IconButton>
                        <IconButton label="Forward" onPress={goForward} disabled={!canGoForward}>
                            <ChevronRightIcon size={iconSize} color={canGoForward ? ACTIVE : DISABLED} />
                        </IconButton>
                        <IconButton label="Search" onPress={startSearch}>
                            <SearchIcon size={iconSize} color={ACTIVE} />
                        </IconButton>
                        <IconButton label={menuOpen ? "Close menu" : "Menu"} onPress={toggleMenu}>
                            {menuOpen ? <XIcon size={iconSize} color={ACTIVE} /> : <MenuIcon size={iconSize} color={ACTIVE} />}
                        </IconButton>
                    </>
                )}
            </View>
        </>
    );
}

const shadow = {
    shadowColor: "#000",
    shadowOpacity: 0.12,
    shadowRadius: 10,
    shadowOffset: { width: 0, height: 4 },
    elevation: 6, // Android shadow
} as const;

const styles = StyleSheet.create({
    bar: {
        position: "absolute", // floats over the folder list
        left: 16,
        right: 16,
        height: BROWSE_BAR_HEIGHT,
        flexDirection: "row",
        justifyContent: "space-around",
        alignItems: "center",
        gap: 8,
        paddingHorizontal: 12,
        backgroundColor: "white",
        borderRadius: 16,
        ...shadow,
    },
    iconButton: {
        padding: 8,
    },
    searchInput: {
        flex: 1,
        height: "100%",
    },
    menu: {
        position: "absolute",
        right: 16,
        minWidth: 200,
        paddingVertical: 6,
        backgroundColor: "white",
        borderRadius: 12,
        ...shadow,
    },
    menuItem: {
        flexDirection: "row",
        alignItems: "center",
        gap: 12,
        paddingHorizontal: 16,
        paddingVertical: 12,
    },
    menuDivider: {
        height: StyleSheet.hairlineWidth,
        backgroundColor: "#C7C7CC",
        marginHorizontal: 12,
    },
});
