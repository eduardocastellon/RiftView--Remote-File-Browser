// modal
// aligned: right
// icon: /assets/icons/add.png

import { router } from "expo-router";
import { useState } from "react";
import { Image, StyleSheet, Text, TextInput, TouchableOpacity } from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";
import { CodeInput } from "../components/code-input";

export default function AddDevice(){
    const [deviceFound, setDeviceFound] = useState<boolean>(false);
    const [authCode, setAuthCode] = useState("");

    return(
        <SafeAreaProvider>
            {!deviceFound ? (
            <SafeAreaView style={styles.content}>
                <Text>Enter the ip address of the host you want to connect to</Text>
                <TextInput
                    style={styles.input}
                    placeholderTextColor="#8E8E93"
                    placeholder="xxx.xxx.xxx.xxx"
                    // keyboardType="decimal-pad"
                    autoCapitalize="none"
                    autoCorrect={false}
                />

                <Text>Name the device</Text>
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
                    <Text>Need help? Click here</Text>
                    <Image source={require("../../assets/icons/info.png")} style={styles.helpIcon} />
                </TouchableOpacity>
            </SafeAreaView>
            ) : (
                <SafeAreaView style={styles.authView}>
                    <Text>Enter authentication code</Text>
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
    content: {
        flex: 1,
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
        justifyContent: "center",
        alignItems: "center",
        gap: 16,
    }
});