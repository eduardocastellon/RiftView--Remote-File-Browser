// tab
// aligned: center
// icon: /assets/icons/devices.png

import DeviceEntry from "@/components/deviceEntry";
import { router } from "expo-router";
import { useState } from "react";
import { Image, ScrollView, StyleSheet, Text, TouchableOpacity } from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";
import { font_size } from "../../../vars";

export default function Index() {
  // Get list of saved devices
  const [deviceList, setDeviceList] = useState<any>(true);

  return (
    <SafeAreaProvider>
      {deviceList ? (
        // if there are devices, show
        <SafeAreaView style={styles.container}>
          <ScrollView>
            <DeviceEntry />


          </ScrollView>
        </SafeAreaView>
      ) : (
        // if no devices, show
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
  // No alignItems: "center" here — that would shrink the ScrollView (and the
  // device rows inside it) to the width of their content.
  container: {
    flex: 1,
    padding: 20,
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
