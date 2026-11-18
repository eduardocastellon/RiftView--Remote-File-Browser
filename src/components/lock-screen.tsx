import { Modal, StyleSheet, Text, TouchableOpacity, View } from "react-native";
import { useFontSize } from "../hooks/font-size";

type Props = {
    visible: boolean;
    /** e.g. "Face ID"; empty if the phone has no lock set up. */
    methodLabel: string;
    error: string | null;
    onUnlock: () => void;
};

/**
 * Covers the whole app while it's locked. A Modal (not a plain View) so it
 * also sits above native sheets like "Connect to a machine".
 */
export default function LockScreen({ visible, methodLabel, error, onUnlock }: Props) {
    const font_size = useFontSize();
    return (
        <Modal visible={visible} animationType="fade" presentationStyle="fullScreen" onRequestClose={() => {}}>
            <View style={styles.container}>
                <Text style={[styles.title, { fontSize: font_size + 6 }]}>RiftView is locked</Text>
                {error && <Text style={[styles.error, { fontSize: font_size }]}>{error}</Text>}
                <TouchableOpacity style={styles.button} onPress={onUnlock}>
                    <Text style={[styles.buttonText, { fontSize: font_size }]}>
                        {methodLabel ? `Unlock with ${methodLabel}` : "Unlock"}
                    </Text>
                </TouchableOpacity>
            </View>
        </Modal>
    );
}

const styles = StyleSheet.create({
    container: {
        flex: 1,
        justifyContent: "center",
        alignItems: "center",
        gap: 20,
        padding: 20,
        backgroundColor: "white",
    },
    title: {
        fontWeight: "bold",
    },
    error: {
        color: "#D70015",
        textAlign: "center",
    },
    button: {
        backgroundColor: "#252525",
        paddingHorizontal: 20,
        paddingVertical: 10,
        borderRadius: 5,
    },
    buttonText: {
        color: "white",
    },
});
