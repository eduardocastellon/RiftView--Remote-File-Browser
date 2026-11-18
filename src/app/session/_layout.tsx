// A browsing session with one host: /session?id=<deviceId>&share=…&path=…
//
// This layout stays mounted while the user moves between folders, so the
// bottom bar and the back/forward history persist. Leaving the session
// (Exit, or back from the share list) unmounts it, which logs out.

import { Stack, useGlobalSearchParams } from "expo-router";
import { useEffect, useState } from "react";
import { View } from "react-native";
import { getHost } from "../../../data/hosts";
import { logout } from "../../api/auth";
import BrowseBar from "../../components/browseBar";
import { SessionHistoryProvider } from "../../hooks/session-history";

export default function SessionLayout() {
    const params = useGlobalSearchParams<{ id: string }>();
    // The host is fixed for the whole session: read it once on entry.
    const [deviceId] = useState(() => params.id);

    // End the session on the host when the user leaves it.
    useEffect(() => {
        return () => {
            getHost(deviceId).then((host) => host && logout(host));
        };
    }, [deviceId]);

    return (
        <SessionHistoryProvider deviceId={deviceId}>
            <View style={{ flex: 1 }}>
                {/* No header back button: the bottom bar handles back / forward. */}
                <Stack screenOptions={{ headerBackVisible: false }} />
                <BrowseBar />
            </View>
        </SessionHistoryProvider>
    );
}
