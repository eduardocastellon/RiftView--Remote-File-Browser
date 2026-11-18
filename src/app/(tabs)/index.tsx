// tab
// aligned: center
// icon: /assets/icons/devices.png

import DeviceEntry from "@/components/deviceEntry";
import { router, useFocusEffect } from "expo-router";
import { useCallback, useState } from "react";
import { Image, ScrollView, StyleSheet, Text, TouchableOpacity } from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";
import { hostDisplayName, listHosts, type HostRecord } from "../../../data/hosts";
import { useFontSize } from "../../hooks/font-size";

export default function Index() {
  const font_size = useFontSize();
  // Get list of saved devices
  const [deviceList, setDeviceList] = useState<HostRecord[]>([]);

  // Reload whenever this tab comes back into view (e.g. after pairing).
  useFocusEffect(
    useCallback(() => {
      listHosts().then(setDeviceList);
    }, []),
  );

  return (
    <SafeAreaProvider>
      {deviceList.length > 0 ? (
        // if there are devices, show
        <SafeAreaView style={styles.container}>
          <ScrollView contentContainerStyle={styles.list}>
            {deviceList.map((host) => (
              <DeviceEntry
                key={host.deviceId}
                name={hostDisplayName(host)}
                address={`${host.address}:${host.port}`}
                onPress={() => router.push({ pathname: "/[id]", params: { id: host.deviceId } })}
              />
            ))}
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
  list: {
    gap: 10,
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
