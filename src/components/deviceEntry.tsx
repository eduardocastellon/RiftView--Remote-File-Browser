import { Image, StyleSheet, Text, TouchableOpacity } from "react-native";
import { font_size } from "../../vars";

// this function will pass in props
export default function DeviceEntry(){
    return(
        <TouchableOpacity style={styles.container}>
            <Image source={require("@/assets/icons/devices.png")} style={styles.img}/>
            {/* displays device's custom_alias | name */}
            <Text style={styles.font}>Device</Text>
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
    img: {
        width: 40,
        height: 40,
    }
})