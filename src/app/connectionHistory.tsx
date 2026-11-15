import { StyleSheet, Text } from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";

export default function ConnectionHistory(){
    return(
        <SafeAreaProvider>
            <SafeAreaView style={styles.content}>
                <Text>This page will display the connection history</Text>
            </SafeAreaView>
        </SafeAreaProvider>
    );
}
const styles = StyleSheet.create({
    content: {
        padding: 20,
    },
});