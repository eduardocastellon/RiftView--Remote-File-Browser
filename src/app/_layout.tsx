// Must load before anything uses crypto.getRandomValues (device keys, SPAKE2).
import "react-native-get-random-values";

import { Stack } from "expo-router";
import { AppLockProvider } from "../hooks/app-lock";
import { FontSizeProvider } from "../hooks/font-size";

export default function RootLayout() {
  return (
    <FontSizeProvider>
    <AppLockProvider>
    <Stack>
      <Stack.Screen name="(tabs)" options={{ headerShown: false }} />
      <Stack.Screen
        name="addDevice"
        options={{
          presentation: "formSheet",
          sheetAllowedDetents: "fitToContents",
          sheetGrabberVisible: true,
          title: "Connect to a machine",
          contentStyle: { backgroundColor: "white" },
        }}
      />
      <Stack.Screen
        name="connectionHistory"
        options={{ title: "Connection History", headerBackButtonDisplayMode: "minimal" }}
      />
      <Stack.Screen
        name="[id]"
        // Title is set by the screen (host, share or folder name).
        options={{ title: "", headerBackButtonDisplayMode: "minimal" }}
      />
    </Stack>
    </AppLockProvider>
    </FontSizeProvider>
  );
}
