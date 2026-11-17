import { Stack } from "expo-router";

export default function RootLayout() {
  return (
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
        name="deviceSession"
        options={{ title: "Session", headerBackButtonDisplayMode: "minimal" }}
      />
    </Stack>
  );
}
