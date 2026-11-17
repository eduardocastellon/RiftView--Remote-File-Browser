import { Image, StyleSheet, Text, TouchableOpacity, View } from "react-native";
import { font_size } from "../../vars";

type Props = {
    /** The host's display name: the user's alias, else the host's own name. */
    name: string;
    /** "address:port", shown under the name. */
    address: string;
    onPress?: () => void;
};

export default function DeviceEntry({ name, address, onPress }: Props){
    return(
        <TouchableOpacity style={styles.container} onPress={onPress}>
            <Image source={require("@/assets/icons/devices.png")} style={styles.img}/>
            <View>
                <Text style={styles.font}>{name}</Text>
                <Text style={styles.address}>{address}</Text>
            </View>
        </TouchableOpacity>
    );
};

const styles = StyleSheet.create({
    container: {
        flexDirection: "row",
        alignItems: "center",
        gap: 15,
        borderWidth: 1,
        borderColor: "black",
        width: "100%",
        padding: 5,
    },
    font:{
        fontSize: font_size,
    },
    address: {
        color: "#6C6C70",
    },
    img: {
        width: 40,
        height: 40,
    }
})
