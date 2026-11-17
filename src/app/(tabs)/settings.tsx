// tab
// aligned: left
// icon: /assets/icons/cog.png

import { Host, Slider } from "@expo/ui";
import { router } from "expo-router";
import { useState } from "react";
import { Alert, Linking, ScrollView, StyleSheet, Switch, Text, TouchableOpacity, View } from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";
import { buildNumber, privacyPolicyUrl, sourceCodeUrl, version } from "../../../strings";
import { font_size } from "../../../vars";
import { forgetAllHosts } from "../../api/devices";

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
    const [requireFaceID, setRequireFaceID] = useState<boolean>(false);
    // UI only for now: moves the slider but isn't wired to the text size yet.
    // Snaps to 4 positions (0-3), matching small / regular / large / xlarge
    // in vars.ts. Starts on 1 (regular).
    const [textSize, setTextSize] = useState<number>(1);
    const textSizeDisplay: string = textSize === 0 ? "small" : textSize === 1 ? "regular" : textSize === 2 ? "large" : textSize === 3 ? "extra large" : "Error";

    return(
        <SafeAreaProvider>
            <SafeAreaView style={styles.screen}>
                <ScrollView contentContainerStyle={styles.content}>
                {/* appearance */}
                <View style={styles.section}>
                    <Text style={styles.header}>Appearance</Text>
                    <View style={styles.subText}>
                        <Text style={{ fontSize: font_size }}>Text Size:</Text>
                        <Text style={{ fontSize: font_size }}>{textSizeDisplay}</Text>
                    </View>
                    
                    <Host matchContents={{ vertical: true }} style={styles.slider}>
                        <Slider value={textSize} onValueChange={setTextSize} min={0} max={3} step={1} />
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
                    <Text style={styles.header}>Security & Privacy</Text>
                     <View style={styles.subText}>
                        <Text style={{ fontSize: font_size }}>Forget all connections</Text><TouchableOpacity onPress={confirmForgetAll}><Text style={{ fontSize: font_size }}>Clear</Text></TouchableOpacity>
                    </View>
                    <View style={styles.subText}>
                        <Text style={{ fontSize: font_size }}>View connection history</Text><TouchableOpacity onPress={()=>router.push("/connectionHistory")}><Text style={{ fontSize: font_size }}>View</Text></TouchableOpacity>
                    </View>
                    <View style={styles.subText}>
                        <Text style={{ fontSize: font_size }}>Require Face ID</Text>
                        <Switch
                            trackColor={{ false: '#767577', true: '#81b0ff' }}
                            thumbColor={requireFaceID ? '#007AFF' : '#f4f3f4'}
                            ios_backgroundColor="#3e3e3e"
                            onValueChange={()=>setRequireFaceID(!requireFaceID)}
                            value={requireFaceID}
                        />
                    </View>
                </View>

                {/* about */}
                <View style={styles.section}>
                    <Text style={styles.header}>About</Text>
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
        fontSize: font_size,
    },
    slider: {
        width: "100%",
    },
    subText: {
        flexDirection: "row",
        justifyContent: "space-between",
        alignItems: "center",
        // borderWidth: 1,
        // borderColor: "black"
    }
});