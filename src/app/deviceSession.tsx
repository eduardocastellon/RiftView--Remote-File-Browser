// Connects to a paired host: logs in (client_reqs.md §8) and shows what the
// host reports. Browsing files will build on this screen.

import { router, useLocalSearchParams } from "expo-router";
import { useEffect, useState } from "react";
import { ActivityIndicator, Alert, ScrollView, StyleSheet, Text, TouchableOpacity, View } from "react-native";
import { hostDisplayName, type HostRecord } from "../../data/hosts";
import { font_size } from "../../vars";
import { DeviceRemovedError } from "../api/auth";
import { forgetHost, getHost, getHostInfo, getMe, type DeviceMe, type HostInfo } from "../api/devices";

type State =
    | { kind: "loading" }
    | { kind: "connected"; info: HostInfo; me: DeviceMe }
    | { kind: "error"; message: string; removed: boolean };

export default function DeviceSession(){
    const { deviceId } = useLocalSearchParams<{ deviceId: string }>();
    const [host, setHost] = useState<HostRecord | null>(null);
    const [state, setState] = useState<State>({ kind: "loading" });
    const [attempt, setAttempt] = useState(0);

    useEffect(() => {
        let cancelled = false;
        (async () => {
            setState({ kind: "loading" });
            const record = await getHost(deviceId);
            if (cancelled) return;
            if (!record) {
                setState({ kind: "error", message: "This host isn't saved on this phone anymore.", removed: true });
                return;
            }
            setHost(record);
            try {
                const [info, me] = await Promise.all([getHostInfo(record), getMe(record)]);
                if (!cancelled) setState({ kind: "connected", info, me });
            } catch (e) {
                if (!cancelled) {
                    setState({ kind: "error", message: (e as Error).message, removed: e instanceof DeviceRemovedError });
                }
            }
        })();
        return () => {
            cancelled = true;
        };
    }, [deviceId, attempt]);

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
                        router.back();
                    },
                },
            ],
        );
    }

    return(
        <ScrollView contentContainerStyle={styles.content}>
            {host && <Text style={styles.title}>{hostDisplayName(host)}</Text>}
            {host && <Text style={styles.muted}>{host.address}:{host.port}</Text>}

            {state.kind === "loading" && <ActivityIndicator />}

            {state.kind === "connected" && (
                <View style={styles.section}>
                    <Row label="Host" value={state.info.name} />
                    <Row label="System" value={`${state.info.os} (${state.info.arch})`} />
                    <Row label="RiftView" value={state.info.version} />
                    <Row label="This phone" value={state.me.name} />
                    <Row label="Permissions" value={state.me.permissions.join(", ") || "none"} />
                </View>
            )}

            {state.kind === "error" && (
                <View style={styles.section}>
                    <Text style={styles.error}>{state.message}</Text>
                    {!state.removed && (
                        <TouchableOpacity style={styles.button} onPress={() => setAttempt((n) => n + 1)}>
                            <Text style={styles.buttonText}>Try again</Text>
                        </TouchableOpacity>
                    )}
                </View>
            )}

            {host && (
                <TouchableOpacity onPress={confirmForget}>
                    <Text style={styles.destructive}>Forget this host</Text>
                </TouchableOpacity>
            )}
        </ScrollView>
    );
};

function Row({ label, value }: { label: string; value: string }) {
    return (
        <View style={styles.row}>
            <Text style={{ fontSize: font_size }}>{label}</Text>
            <Text style={[styles.muted, { fontSize: font_size }]}>{value}</Text>
        </View>
    );
}

const styles = StyleSheet.create({
    content: {
        padding: 20,
        gap: 16,
    },
    title: {
        fontSize: font_size + 4,
        fontWeight: "bold",
    },
    section: {
        gap: 8,
    },
    row: {
        flexDirection: "row",
        justifyContent: "space-between",
        minHeight: 32,
        alignItems: "center",
    },
    muted: {
        color: "#6C6C70",
    },
    error: {
        color: "#D70015",
        fontSize: font_size,
    },
    button: {
        alignSelf: "flex-start",
        backgroundColor: "#252525",
        paddingHorizontal: 16,
        paddingVertical: 8,
        borderRadius: 5,
    },
    buttonText: {
        color: "white",
    },
    destructive: {
        color: "#D70015",
        fontSize: font_size,
    },
});
