// modal
// aligned: right
// icon: /assets/icons/add.png

import { router } from "expo-router";
import { useEffect, useRef, useState } from "react";
import { ActivityIndicator, Image, StyleSheet, Text, TextInput, TouchableOpacity } from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";
import { hostDisplayName } from "../../data/hosts";
import { PairStatus } from "../../vars";
import { DEFAULT_PORT } from "../api/paths";
import { PairingError, pairWithHost, probeHost } from "../api/pairing";
import { CodeInput } from "../components/code-input";
import { useFontSize } from "../hooks/font-size";

type Step = "address" | "code" | "waiting" | "done";

/**
 * Accepts "192.168.1.20", "192.168.1.20:7878" or "https://192.168.1.20:7878"
 * (IPv4 or hostname; an optional trailing "/" is ignored). The host only
 * speaks HTTPS, so "https://" is the only scheme allowed.
 */
function parseAddress(input: string): { address: string; port: number } | null {
    const withoutScheme = input.trim().replace(/^https:\/\//i, "").replace(/\/$/, "");
    const match = withoutScheme.match(/^([^\s:/]+)(?::(\d{1,5}))?$/);
    if (!match) return null;
    const port = match[2] ? Number(match[2]) : DEFAULT_PORT;
    if (port < 1 || port > 65535) return null;
    return { address: match[1], port };
}

export default function AddDevice(){
    const font_size = useFontSize();
    const [step, setStep] = useState<Step>("address");
    const [status, setStatus] = useState<PairStatus>(PairStatus.IDLE);
    const [error, setError] = useState<string | null>(null);

    const [addressInput, setAddressInput] = useState("");
    const [alias, setAlias] = useState("");
    const [target, setTarget] = useState<{ address: string; port: number; seenPin: string } | null>(null);

    const [authCode, setAuthCode] = useState("");
    const [codeInputKey, setCodeInputKey] = useState(0); // bump to clear the boxes
    const [fingerprint, setFingerprint] = useState<string | null>(null);
    const [pairedName, setPairedName] = useState<string | null>(null);

    const abortRef = useRef<AbortController | null>(null);
    // Stop polling if the sheet is closed mid-pairing.
    useEffect(() => () => abortRef.current?.abort(), []);

    const busy = status === PairStatus.SCANNING || status === PairStatus.CONNECTING || status === PairStatus.AUTHENTICATING;

    async function connect() {
        const parsed = parseAddress(addressInput);
        if (!parsed) {
            setError(
                /^http:\/\//i.test(addressInput.trim())
                    ? "RiftView hosts use https://, not http://."
                    : "Enter an address like 192.168.1.20 or https://192.168.1.20:7878.",
            );
            return;
        }
        setError(null);
        setStatus(PairStatus.SCANNING);
        try {
            const seenPin = await probeHost(parsed.address, parsed.port);
            setTarget({ ...parsed, seenPin });
            setStatus(PairStatus.IDLE);
            setStep("code");
        } catch (e) {
            setStatus(PairStatus.NOT_FOUND);
            setError((e as Error).message);
        }
    }

    async function pair() {
        if (!target) return;
        setError(null);
        setStatus(PairStatus.CONNECTING);
        const controller = new AbortController();
        abortRef.current = controller;
        try {
            const host = await pairWithHost({
                ...target,
                code: authCode,
                alias,
                signal: controller.signal,
                onProgress: (p) => {
                    if (p.stage === "awaiting_approval") {
                        setFingerprint(p.fingerprint);
                        setStatus(PairStatus.AUTHENTICATING);
                        setStep("waiting");
                    }
                },
            });
            setPairedName(hostDisplayName(host));
            setStatus(PairStatus.SUCCESS);
            setStep("done");
        } catch (e) {
            if (e instanceof PairingError && e.reason === "aborted") return;
            setStatus(e instanceof PairingError && e.reason === "expired" ? PairStatus.TIMEOUT : PairStatus.FAILED);
            setError((e as Error).message);
            // Codes are single-use: go back to the code step with empty boxes.
            setAuthCode("");
            setCodeInputKey((k) => k + 1);
            setStep("code");
        } finally {
            abortRef.current = null;
        }
    }

    function cancelPairing() {
        abortRef.current?.abort();
        setStatus(PairStatus.IDLE);
        setAuthCode("");
        setCodeInputKey((k) => k + 1);
        setStep("code");
    }

    const errorText = error ? <Text style={styles.error}>{error}</Text> : null;

    return(
        <SafeAreaProvider>
            {step === "address" && (
            <SafeAreaView style={styles.content}>
                <Text style={{ fontSize: font_size }}>Enter the ip address of the host you want to connect to</Text>
                <TextInput
                    style={styles.input}
                    placeholderTextColor="#8E8E93"
                    placeholder="xxx.xxx.xxx.xxx"
                    // keyboardType="decimal-pad"
                    autoCapitalize="none"
                    autoCorrect={false}
                    value={addressInput}
                    onChangeText={setAddressInput}
                    editable={!busy}
                />

                <Text style={{ fontSize: font_size }}>Name the device</Text>
                <TextInput
                    style={styles.input}
                    placeholderTextColor="#8E8E93"
                    placeholder="Enter a name for this device"
                    // keyboardType="decimal-pad"
                    autoCapitalize="none"
                    autoCorrect={false}
                    value={alias}
                    onChangeText={setAlias}
                    editable={!busy}
                />

                {errorText}

                <TouchableOpacity style={[styles.button, busy && styles.buttonDisabled]} onPress={connect} disabled={busy}>
                    {status === PairStatus.SCANNING
                        ? <ActivityIndicator color="white" />
                        : <Text style={styles.buttonText}>Connect</Text>}
                </TouchableOpacity>

                <TouchableOpacity style={styles.help}>
                    <Text style={{ fontSize: font_size }}>Need help? Click here</Text>
                    <Image source={require("../../assets/icons/info.png")} style={styles.helpIcon} />
                </TouchableOpacity>
            </SafeAreaView>
            )}

            {step === "code" && (
                <SafeAreaView style={styles.authView}>
                    <Text style={{ fontSize: font_size }}>Enter authentication code</Text>
                    <Text style={styles.hint}>Run `riftview-api pair` on the host to get one.</Text>
                    <CodeInput key={codeInputKey} onChange={setAuthCode} />

                    {errorText}

                    <TouchableOpacity
                        style={[styles.button, (busy || authCode.length !== 6) && styles.buttonDisabled]}
                        onPress={pair}
                        disabled={busy || authCode.length !== 6}
                    >
                        {status === PairStatus.CONNECTING
                            ? <ActivityIndicator color="white" />
                            : <Text style={styles.buttonText}>Pair</Text>}
                    </TouchableOpacity>
                </SafeAreaView>
            )}

            {step === "waiting" && (
                <SafeAreaView style={styles.authView}>
                    <ActivityIndicator />
                    <Text style={{ fontSize: font_size, textAlign: "center" }}>Approve this phone on the host</Text>
                    <Text style={styles.hint}>Check that the host shows the same fingerprint:</Text>
                    <Text style={[styles.fingerprint, { fontSize: font_size }]}>{fingerprint}</Text>
                    <TouchableOpacity style={styles.button} onPress={cancelPairing}>
                        <Text style={styles.buttonText}>Cancel</Text>
                    </TouchableOpacity>
                </SafeAreaView>
            )}

            {step === "done" && (
                <SafeAreaView style={styles.authView}>
                    <Text style={{ fontSize: font_size, textAlign: "center" }}>Paired with {pairedName}</Text>
                    <TouchableOpacity style={styles.button} onPress={() => router.back()}>
                        <Text style={styles.buttonText}>Done</Text>
                    </TouchableOpacity>
                </SafeAreaView>
            )}
        </SafeAreaProvider>

    )
}

const styles = StyleSheet.create({
    // No flex: 1 here: the sheet uses "fitToContents", so it sizes itself
    // to this content's natural height.
    content: {
        padding: 10,
        paddingBottom: 24,
        flexDirection: "column",
        alignItems: "center",
        gap: 10,
    },
    input: {
        width: "80%",
        borderWidth: 1,
        borderColor: "#C7C7CC",
        borderRadius: 8,
        paddingHorizontal: 12,
        paddingVertical: 10,
    },
    button: {
        width: 120,
        height: 35,
        backgroundColor: "#252525",
        padding: 8,
        borderRadius: 5,
        alignItems: "center",
        justifyContent: "center",

    },
    buttonDisabled: {
        opacity: 0.4,
    },
    buttonText: {
        color: "white",
    },
    help: {
        flexDirection: "row",
        alignItems: "center",
        gap: 6,
    },
    helpIcon: {
        width: 16,
        height: 16,
    },
    authView: {
        padding: 10,
        paddingBottom: 24,
        justifyContent: "center",
        alignItems: "center",
        gap: 16,
    },
    hint: {
        color: "#6C6C70",
        textAlign: "center",
        paddingHorizontal: 20,
    },
    fingerprint: {
        fontFamily: "Menlo",
        fontWeight: "600",
    },
    error: {
        color: "#D70015",
        textAlign: "center",
        paddingHorizontal: 20,
    },
});
