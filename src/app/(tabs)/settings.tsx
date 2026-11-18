// tab
// aligned: left
// icon: /assets/icons/cog.png

import { Host, Slider } from "@expo/ui";
import { router } from "expo-router";
import { useState } from "react";
import { Alert, Linking, ScrollView, StyleSheet, Switch, Text, TouchableOpacity, View } from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";
import { buildNumber, privacyPolicyUrl, sourceCodeUrl, version } from "../../../strings";
import { font_size_levels } from "../../../vars";
import { forgetAllHosts } from "../../api/devices";
import { useAppLock } from "../../hooks/app-lock";
import { useFontSize, useFontSizeSetting } from "../../hooks/font-size";

function confirmForgetAll() {
    Alert.alert(
        "Forget all connections?",
        "Every paired host is removed from this phone, along with its key. You'll need new codes to pair again.",
        [
            { text: "Cancel", style: "cancel" },
            { text: "Forget all", style: "destructive", onPress: () => void forgetAllHosts() },
        ],
    );
}

export default function Settings(){
    const [themeLight, setThemeLight] = useState<boolean>(true);
    // App lock: Face ID / Touch ID / fingerprint, else the device passcode.
    const appLock = useAppLock();
    const [lockBusy, setLockBusy] = useState(false);
    async function toggleAppLock(next: boolean) {
        setLockBusy(true);
        const error = await appLock.setEnabled(next);
        setLockBusy(false);
        if (error) Alert.alert("Couldn't change app lock", error);
    }
    // "Face ID" / "Touch ID" / "Fingerprint" / "Passcode"
    const lockLabel = appLock.method.label.charAt(0).toUpperCase() + appLock.method.label.slice(1);
    // Snaps to 4 positions (0-3), matching small / regular / large / xlarge
    // in vars.ts. Changing it resizes text across the app and is saved.
    const font_size = useFontSize();
    const { level, setLevel } = useFontSizeSetting();
    const textSize = font_size_levels.indexOf(level);
    const textSizeDisplay: string = textSize === 0 ? "small" : textSize === 1 ? "regular" : textSize === 2 ? "large" : textSize === 3 ? "extra large" : "Error";
    function onTextSizeChange(value: number) {
        const next = font_size_levels[Math.round(value)];
        if (next && next !== level) setLevel(next);
    }

    return(
        <SafeAreaProvider>
            <SafeAreaView style={styles.screen}>
                <ScrollView contentContainerStyle={styles.content}>
                {/* appearance */}
                <View style={styles.section}>
                    <Text style={[styles.header, { fontSize: font_size }]}>Appearance</Text>
                    <View style={styles.subText}>
                        <Text style={{ fontSize: font_size }}>Text Size:</Text>
                        <Text style={{ fontSize: font_size }}>{textSizeDisplay}</Text>
                    </View>
                    
                    <Host matchContents={{ vertical: true }} style={styles.slider}>
                        <Slider value={textSize} onValueChange={onTextSizeChange} min={0} max={3} step={1} />
                    </Host>

                    <View style={styles.subText}>
                        <Text style={{ fontSize: font_size }}>Theme: {themeLight ? "Light" : "Dark"}</Text>
                        <Switch
                            trackColor={{ false: '#767577', true: '#81b0ff' }}
                            thumbColor={themeLight ? '#007AFF' : '#f4f3f4'}
                            ios_backgroundColor="#3e3e3e"
                            onValueChange={()=>setThemeLight(!themeLight)}
                            value={themeLight}
                        />
                    </View>
                </View>

                {/* security & privacy */}
                <View style={styles.section}>
                    <Text style={[styles.header, { fontSize: font_size }]}>Security & Privacy</Text>
                     <View style={styles.subText}>
                        <Text style={{ fontSize: font_size }}>Forget all connections</Text><TouchableOpacity onPress={confirmForgetAll}><Text style={{ fontSize: font_size }}>Clear</Text></TouchableOpacity>
                    </View>
                    <View style={styles.subText}>
                        <Text style={{ fontSize: font_size }}>View connection history</Text><TouchableOpacity onPress={()=>router.push("/connectionHistory")}><Text style={{ fontSize: font_size }}>View</Text></TouchableOpacity>
                    </View>

                    {/* Offers Face ID / Touch ID / fingerprint if set up on the phone,
                        else the device passcode. Phones with no lock can't use it. */}
                    {appLock.method.kind !== "none" ? (
                        <View style={styles.subText}>
                            <Text style={{ fontSize: font_size }}>Require {lockLabel}</Text>
                            <Switch
                                trackColor={{ false: '#767577', true: '#81b0ff' }}
                                thumbColor={appLock.enabled ? '#007AFF' : '#f4f3f4'}
                                ios_backgroundColor="#3e3e3e"
                                onValueChange={toggleAppLock}
                                value={appLock.enabled}
                                disabled={lockBusy}
                            />
                        </View>
                    ) : (
                        <Text style={[styles.note, { fontSize: font_size - 2 }]}>
                            Set up Face ID, a fingerprint or a passcode on this phone to lock RiftView.
                        </Text>
                    )}

                </View>

                {/* about */}
                <View style={styles.section}>
                    <Text style={[styles.header, { fontSize: font_size }]}>About</Text>
                    <View style={styles.subText}>
                        <Text style={{ fontSize: font_size}}>Version</Text><Text style={{ fontSize: font_size }}>{version}</Text>
                    </View>
                    <View style={styles.subText}>
                        <Text style={{ fontSize: font_size}}>Build Number</Text><Text style={{ fontSize: font_size }}>{buildNumber}</Text>
                    </View>
                    <View style={styles.subText}>
                        <Text style={{ fontSize: font_size}}>Privacy Policy</Text><TouchableOpacity onPress={() => Linking.openURL(privacyPolicyUrl)}><Text style={{ fontSize: font_size}}>Read</Text></TouchableOpacity></View>
                    <View style={styles.subText}>
                        <Text style={{ fontSize: font_size}}>Source Code</Text><TouchableOpacity onPress={() => Linking.openURL(sourceCodeUrl)}><Text style={{ fontSize: font_size}}>View</Text></TouchableOpacity>
                    </View>
                </View>

                </ScrollView>
            </SafeAreaView>
        </SafeAreaProvider>

    )
}
const styles = StyleSheet.create({
    screen: {
        flex: 1,
    },
    // Styles the scrollable content itself. No flex: 1 here, or the content
    // would be capped at the screen height and have nothing to scroll.
    content: {
        padding: 20,
        paddingBottom: 70,
        flexDirection: "column",
        gap: 25,
    },
    section: {
        flexDirection: "column",
        gap: 20,
    },
    header: {
        fontWeight: "bold",
    },
    slider: {
        width: "100%",
    },
    note: {
        color: "#6C6C70",
    },
    subText: {
        flexDirection: "row",
        justifyContent: "space-between",
        alignItems: "center",
        // borderWidth: 1,
        // borderColor: "black"
    }
});