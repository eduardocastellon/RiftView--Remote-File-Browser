// modal
// aligned: right
// icon: /assets/icons/add.png

import { router } from "expo-router";
import { useState } from "react";
import { Image, StyleSheet, Text, TextInput, TouchableOpacity } from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";
import { font_size } from "../../vars";
import { CodeInput } from "../components/code-input";

export default function AddDevice(){
    const [deviceFound, setDeviceFound] = useState<boolean>(false);
    const [authCode, setAuthCode] = useState("");

    return(
        <SafeAreaProvider>
            {!deviceFound ? (
            <SafeAreaView style={styles.content}>
                <Text style={{ fontSize: font_size }}>Enter the ip address of the host you want to connect to</Text>
                <TextInput
                    style={styles.input}
                    placeholderTextColor="#8E8E93"
                    placeholder="xxx.xxx.xxx.xxx"
                    // keyboardType="decimal-pad"
                    autoCapitalize="none"
                    autoCorrect={false}
                />

                <Text style={{ fontSize: font_size }}>Name the device</Text>
                <TextInput
                    style={styles.input}
                    placeholderTextColor="#8E8E93"
                    placeholder="Enter a name for this device"
                    // keyboardType="decimal-pad"
                    autoCapitalize="none"
                    autoCorrect={false}
                />

                <TouchableOpacity style={styles.button} onPress={()=>setDeviceFound(true)}>
                    <Text style={styles.buttonText}>Connect</Text>
                </TouchableOpacity>

                <TouchableOpacity style={styles.help}>
                    <Text style={{ fontSize: font_size }}>Need help? Click here</Text>
                    <Image source={require("../../assets/icons/info.png")} style={styles.helpIcon} />
                </TouchableOpacity>
            </SafeAreaView>
            ) : (
                <SafeAreaView style={styles.authView}>
                    <Text style={{ fontSize: font_size }}>Enter authentication code</Text>
                    <CodeInput onChange={setAuthCode} />

                    <TouchableOpacity style={styles.button} onPress={() => router.back()}>
                        <Text style={styles.buttonText}>Pair</Text>
                    </TouchableOpacity>
                    {/* <Text>{authCode}</Text> */}
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
    }
});