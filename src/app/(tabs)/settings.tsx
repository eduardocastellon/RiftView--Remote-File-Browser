// tab
// aligned: left
// icon: /assets/icons/cog.png

import { router } from "expo-router";
import { useState } from "react";
import { Linking, StyleSheet, Switch, Text, TouchableOpacity, View } from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";
import { buildNumber, privacyPolicyUrl, sourceCodeUrl, version } from "../../../strings";

export default function Settings(){
    const [themeLight, setThemeLight] = useState<boolean>(true);
    const [requireFaceID, setRequireFaceID] = useState<boolean>(false);

    return(
        <SafeAreaProvider>
            <SafeAreaView style={styles.content}>
                {/* appearance */}
                <View style={styles.section}>
                    <Text style={styles.header}>Appearance</Text>
                    <View style={styles.subText}>
                        <Text>Theme: {themeLight ? "Light" : "Dark"}</Text>
                        <Switch
                            trackColor={{ false: '#767577', true: '#81b0ff' }}
                            thumbColor={themeLight ? '#007AFF' : '#f4f3f4'}
                            ios_backgroundColor="#3e3e3e"
                            onValueChange={()=>setThemeLight(!themeLight)}
                            value={themeLight}
                        />
                    </View>
                    <View style={styles.subText}>
                        <Text>Forget all connections</Text><TouchableOpacity><Text>Clear</Text></TouchableOpacity>
                    </View>
                    <View style={styles.subText}>
                        <Text>View connection history</Text><TouchableOpacity onPress={()=>router.push("/connectionHistory")}><Text>View</Text></TouchableOpacity>
                    </View>
                </View>

                {/* security & privacy */}
                <View style={styles.section}>
                    <Text style={styles.header}>Security & Privacy</Text>
                    <View style={styles.subText}>
                        <Text>Require Face ID</Text>
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
                        <Text>Version</Text><Text>{version}</Text>
                    </View>
                    <View style={styles.subText}>
                        <Text>Build Number</Text><Text>{buildNumber}</Text>
                    </View>
                    <View style={styles.subText}>
                        <Text>Privacy Policy</Text><TouchableOpacity onPress={() => Linking.openURL(privacyPolicyUrl)}><Text>Read</Text></TouchableOpacity></View>
                    <View style={styles.subText}>
                        <Text>Source Code</Text><TouchableOpacity onPress={() => Linking.openURL(sourceCodeUrl)}><Text>View</Text></TouchableOpacity>
                    </View>
                </View>
            </SafeAreaView>
        </SafeAreaProvider>

    )
}
const styles = StyleSheet.create({
    content: {
        flex: 1,
        padding: 20,
        flexDirection: "column",
        gap: 20,
    },
    section: {
        flexDirection: "column",
        gap: 10,
    },
    header: {
        fontWeight: "bold",
    },
    subText: {
        flexDirection: "row",
        justifyContent: "space-between",
        alignItems: "center",
    }
});