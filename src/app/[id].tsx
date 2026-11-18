// File browser for a paired host (client_reqs.md §9).
//
// Route: /[id]?share=<shareId>&path=<folder path>&title=<header title>
//  - id:    the host's deviceId (which saved host to talk to)
//  - share: missing → this screen lists the host's shares
//  - path:  folder inside the share, default "/"
//  - title: header title (share or folder name), so it shows before loading
//
// Opening a folder pushes this same route with new params, so the back
// button / swipe goes up one folder, and each level keeps its own scroll
// position and data.

import { router, useLocalSearchParams, useNavigation } from "expo-router";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ActivityIndicator, Alert, FlatList, RefreshControl, StyleSheet, Text, TouchableOpacity, View } from "react-native";
import { hostDisplayName, type HostRecord } from "../../data/hosts";
import { DeviceRemovedError } from "../api/auth";
import { ApiError, ConnectionError } from "../api/client";
import { forgetHost, getHost, getMe } from "../api/devices";
import { listFolder, listShares, PAGE_SIZE, type FileEntry as Entry, type Share } from "../api/files";
import FileEntry, { formatBytes, formatDate } from "../components/fileEntry";
import { useFontSize } from "../hooks/font-size";

type Params = { id: string; share?: string; path?: string; title?: string };

/** A row on screen: either a share (at the top level) or a folder entry. */
type Row =
    | { kind: "share"; share: Share }
    | { kind: "entry"; entry: Entry };

type LoadError = { message: string; removed: boolean };

function describeError(e: unknown): LoadError {
    if (e instanceof DeviceRemovedError) return { message: e.message, removed: true };
    if (e instanceof ConnectionError) return { message: e.message, removed: false };
    if (e instanceof ApiError) {
        switch (e.status) {
            case 403: return { message: "This phone isn't allowed to browse files on this host.", removed: false };
            case 404: return { message: "This folder no longer exists on the host.", removed: false };
            case 409: return { message: "This folder has too many items to show.", removed: false };
            case 429: return { message: "Too many requests. Wait a minute and try again.", removed: false };
            case 503: return { message: "This share's folder is missing on the host (e.g. an unplugged drive).", removed: false };
        }
    }
    return { message: (e as Error).message || "Something went wrong.", removed: false };
}

export default function FileBrowser() {
    const { id, share, path = "/", title } = useLocalSearchParams<Params>();
    const navigation = useNavigation();
    const font_size = useFontSize();

    const [host, setHost] = useState<HostRecord | null>(null);
    const [rows, setRows] = useState<Row[]>([]);
    const [total, setTotal] = useState(0);
    const [loading, setLoading] = useState(true);
    const [refreshing, setRefreshing] = useState(false);
    const [loadingMore, setLoadingMore] = useState(false);
    const [error, setError] = useState<LoadError | null>(null);

    // Ignore results from a load that a newer load (refresh/retry) replaced.
    const loadId = useRef(0);

    const isShareList = !share;

    useLayoutEffect(() => {
        navigation.setOptions({
            title: title ?? (host ? hostDisplayName(host) : ""),
        });
        // console.log(id);
    }, [navigation, title, host]);

    /** Fetches the shares (top level) or the first page of the folder. */
    const fetchFirstPage = useCallback(async (): Promise<{ record: HostRecord; rows: Row[]; total: number }> => {
        const record = host ?? (await getHost(id));
        if (!record) throw new DeviceRemovedError("This host isn't saved on this phone anymore.");
        if (isShareList) {
            const [me, shares] = await Promise.all([getMe(record), listShares(record)]);
            if (!me.permissions.includes("browse")) throw new ApiError(403, "forbidden", "forbidden");
            return { record, rows: shares.map((s) => ({ kind: "share", share: s })), total: shares.length };
        }
        const page = await listFolder(record, share, { path, offset: 0 });
        return { record, rows: page.entries.map((e) => ({ kind: "entry", entry: e })), total: page.total };
    }, [host, id, isShareList, share, path]);

    /** Runs a load; only the newest one is allowed to update the screen. */
    const runLoad = useCallback(() => {
        const myLoad = ++loadId.current;
        fetchFirstPage()
            .then((result) => {
                if (myLoad !== loadId.current) return;
                setHost(result.record);
                setRows(result.rows);
                setTotal(result.total);
                setError(null);
            })
            .catch((e) => {
                if (myLoad === loadId.current) setError(describeError(e));
            })
            .finally(() => {
                if (myLoad !== loadId.current) return;
                setLoading(false);
                setRefreshing(false);
            });
    }, [fetchFirstPage]);

    // Load once per screen (a new folder is a new screen with new params).
    // eslint-disable-next-line react-hooks/exhaustive-deps
    useEffect(() => runLoad(), [id, share, path]);

    function retry() {
        setError(null);
        setLoading(true);
        runLoad();
    }

    function refresh() {
        setRefreshing(true);
        runLoad();
    }

    async function loadMore() {
        if (isShareList || !host || !share || loadingMore || loading || refreshing || rows.length >= total) return;
        const myLoad = loadId.current;
        setLoadingMore(true);
        try {
            const page = await listFolder(host, share, { path, offset: rows.length, limit: PAGE_SIZE });
            if (myLoad !== loadId.current) return;
            setRows((prev) => [...prev, ...page.entries.map((e): Row => ({ kind: "entry", entry: e }))]);
            setTotal(page.total);
        } catch (e) {
            if (myLoad === loadId.current) setError(describeError(e));
        } finally {
            setLoadingMore(false);
        }
    }

    function openShare(s: Share) {
        router.push({ pathname: "/[id]", params: { id, share: s.id, path: "/", title: s.name } });
    }

    function openEntry(entry: Entry) {
        if (entry.type === "directory") {
            router.push({ pathname: "/[id]", params: { id, share: share!, path: entry.path, title: entry.name } });
            return;
        }
        // Downloading needs streaming support in the native module (§9 "Download").
        const lines = [
            entry.size !== undefined ? `Size: ${formatBytes(entry.size)}` : null,
            entry.mime ? `Type: ${entry.mime}` : null,
            entry.modified_ms !== undefined ? `Modified: ${formatDate(entry.modified_ms)}` : null,
            `Location: ${entry.path}`,
        ].filter(Boolean);
        Alert.alert(entry.name, lines.join("\n"));
    }

    function confirmForget() {
        if (!host) return;
        Alert.alert(
            `Forget ${hostDisplayName(host)}?`,
            "This phone's key for the host is deleted. You'll need a new code from the host to pair again.",
            [
                { text: "Cancel", style: "cancel" },
                {
                    text: "Forget",
                    style: "destructive",
                    onPress: async () => {
                        await forgetHost(host);
                        router.dismissTo("/");
                    },
                },
            ],
        );
    }

    const header = (
        <View style={styles.pathBar}>
            <Text style={styles.pathText} numberOfLines={1} ellipsizeMode="head">
                {isShareList
                    ? host ? `${host.address}:${host.port}` : " "
                    : path}
            </Text>
        </View>
    );

    const forgetButton = host && (isShareList || error?.removed) ? (
        <TouchableOpacity style={styles.forget} onPress={confirmForget}>
            <Text style={[styles.destructive, { fontSize: font_size }]}>Forget this host</Text>
        </TouchableOpacity>
    ) : null;

    if (loading) {
        return (
            <View style={styles.center}>
                <ActivityIndicator />
            </View>
        );
    }

    if (error && rows.length === 0) {
        return (
            <View style={styles.center}>
                <Text style={[styles.error, { fontSize: font_size }]}>{error.message}</Text>
                {!error.removed && (
                    <TouchableOpacity style={styles.button} onPress={retry}>
                        <Text style={styles.buttonText}>Try again</Text>
                    </TouchableOpacity>
                )}
                {forgetButton}
            </View>
        );
    }

    return (
        <FlatList
            style={styles.list}
            data={rows}
            keyExtractor={(row) => (row.kind === "share" ? row.share.id : row.entry.id)}
            renderItem={({ item }) =>
                item.kind === "share" ? (
                    <FileEntry name={item.share.name} type="directory" onPress={() => openShare(item.share)} />
                ) : (
                    <FileEntry
                        name={item.entry.name}
                        type={item.entry.type}
                        size={item.entry.size}
                        modifiedMs={item.entry.modified_ms}
                        mime={item.entry.mime}
                        onPress={() => openEntry(item.entry)}
                    />
                )
            }
            ListHeaderComponent={header}
            ListEmptyComponent={
                <Text style={[styles.empty, { fontSize: font_size }]}>
                    {isShareList ? "This host isn't sharing any folders yet." : "This folder is empty."}
                </Text>
            }
            ListFooterComponent={
                <View style={styles.footer}>
                    {loadingMore && <ActivityIndicator />}
                    {error && <Text style={[styles.error, { fontSize: font_size }]}>{error.message}</Text>}
                    {!isShareList && rows.length > 0 && (
                        <Text style={styles.count}>{total} {total === 1 ? "item" : "items"}</Text>
                    )}
                    {forgetButton}
                </View>
            }
            onEndReached={loadMore}
            onEndReachedThreshold={0.5}
            refreshControl={<RefreshControl refreshing={refreshing} onRefresh={refresh} />}
            contentInsetAdjustmentBehavior="automatic"
        />
    );
}

const styles = StyleSheet.create({
    list: {
        flex: 1,
        backgroundColor: "white",
    },
    center: {
        flex: 1,
        justifyContent: "center",
        alignItems: "center",
        gap: 16,
        padding: 20,
        backgroundColor: "white",
    },
    pathBar: {
        paddingHorizontal: 16,
        paddingVertical: 8,
        backgroundColor: "#F2F2F7",
    },
    pathText: {
        color: "#6C6C70",
        fontFamily: "Menlo",
    },
    empty: {
        color: "#6C6C70",
        textAlign: "center",
        marginTop: 40,
    },
    footer: {
        alignItems: "center",
        gap: 12,
        paddingVertical: 20,
    },
    count: {
        color: "#6C6C70",
    },
    error: {
        color: "#D70015",
        textAlign: "center",
    },
    button: {
        backgroundColor: "#252525",
        paddingHorizontal: 16,
        paddingVertical: 8,
        borderRadius: 5,
    },
    buttonText: {
        color: "white",
    },
    forget: {
        paddingVertical: 8,
    },
    destructive: {
        color: "#D70015",
    },
});
