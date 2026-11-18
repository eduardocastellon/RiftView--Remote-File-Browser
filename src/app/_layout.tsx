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
        name="session"
        // The session has its own stack and headers (session/_layout.tsx).
        // No swipe back to the device list (leave via the bar's Exit); swiping
        // between folders inside the session still works.
        options={{ headerShown: false, gestureEnabled: false }}
      />
    </Stack>
    </AppLockProvider>
    </FontSizeProvider>
  );
}
