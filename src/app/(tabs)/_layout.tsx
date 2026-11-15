import { Tabs, router } from "expo-router";
import {
    Easing,
    Image,
    useWindowDimensions,
    type ColorValue,
    type ImageSourcePropType,
} from "react-native";

// Open on the center "Devices" tab even though Settings is listed first.
export const unstable_settings = { initialRouteName: "index" };

function TabIcon({ source, color }: { source: ImageSourcePropType; color: ColorValue }) {
  return <Image source={source} style={{ width: 24, height: 24, tintColor: color }} />;
}

export default function TabLayout() {
  const { width } = useWindowDimensions();

  return (
    <Tabs
      screenOptions={{
        tabBarActiveTintColor: "#007AFF",
        // Full-width push between tabs, direction based on tab order.
        // `progress` is -1 for tabs left of the active one, 0 for the active
        // tab, and 1 for tabs to its right. So switching to Settings (left)
        // slides it in from the left, and switching to Devices (to its right)
        // slides it in from the right.
        sceneStyleInterpolator: ({ current }) => ({
          sceneStyle: {
            transform: [
              {
                translateX: current.progress.interpolate({
                  inputRange: [-1, 0, 1],
                  outputRange: [-width, 0, width],
                }),
              },
            ],
          },
        }),
        transitionSpec: {
          animation: "timing",
          config: { duration: 300, easing: Easing.out(Easing.cubic) },
        },
      }}
    >
      {/* left */}
      <Tabs.Screen
        name="settings"
        options={{
          title: "Settings",
          tabBarIcon: ({ color }) => (
            <TabIcon source={require("../../../assets/icons/cog.png")} color={color} />
          ),
        }}
      />
      {/* center */}
      <Tabs.Screen
        name="index"
        options={{
          title: "Devices",
          tabBarIcon: ({ color }) => (
            <TabIcon source={require("../../../assets/icons/devices.png")} color={color} />
          ),
        }}
      />
      {/* right — opens the addDevice modal instead of switching tabs */}
      <Tabs.Screen
        name="add"
        options={{
          title: "Add",
          tabBarIcon: ({ color }) => (
            <TabIcon source={require("../../../assets/icons/add.png")} color={color} />
          ),
        }}
        listeners={{
          tabPress: (e) => {
            e.preventDefault();
            router.push("/addDevice");
          },
        }}
      />
    </Tabs>
  );
}
