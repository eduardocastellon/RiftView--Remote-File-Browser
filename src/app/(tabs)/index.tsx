// tab
// aligned: center
// icon: /assets/icons/devices.png

import { router } from "expo-router";
import { useState } from "react";
import { Image, StyleSheet, Text, TouchableOpacity } from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";
import { font_size } from "../../../vars";

export default function Index() {
  // Get list of saved devices
  const [deviceList, setDeviceList] = useState<any>(false);

  return (
    <SafeAreaProvider>
      {deviceList ? (
        <SafeAreaView style={styles.container}>
          <Text>Home page</Text>

        
        </SafeAreaView>
      ) : (
        <SafeAreaView style={styles.noDeviceContainer}>
          <Image source={require("../../../assets/icons/nodevicesfound.png")} style={styles.noDeviceImg}/>
          <Text style={{fontSize: font_size, textAlign: "center"}}>It looks like you don't have any machines yet :/</Text>
          <Text style={{fontSize: font_size, marginTop: -15}}>Add one?</Text>
          <TouchableOpacity onPress={() => router.push("/addDevice")}>
            <Image source={require("../../../assets/icons/add.png")} style={styles.addDevice}/>
          </TouchableOpacity>
        </SafeAreaView>
      )}
      
    </SafeAreaProvider>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    padding: 20,
    alignItems: "center",
    justifyContent: "center",
  },
  noDeviceContainer: {
    flex: 1,
    flexDirection: "column",
    justifyContent: "center",
    alignItems: "center",
    gap: 30,
  },
  noDeviceImg: {
    width: 60,
    height: 60,
  },
  addDevice: {
    width: 60,
    height: 60,
  }
});
