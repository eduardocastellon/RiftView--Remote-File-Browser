import { Image, StyleSheet, Text, TouchableOpacity, View, type ImageSourcePropType } from "react-native";
import { useFontSize } from "../hooks/font-size";

const icons = {
    folder: require("@/assets/browse/folder.png"),
    file: require("@/assets/browse/file.png"),
    document: require("@/assets/browse/document.png"),
    image: require("@/assets/browse/image.png"),
    pdf: require("@/assets/browse/pdf.png"),
    slides: require("@/assets/browse/slides.png"),
    spreadsheet: require("@/assets/browse/spreadsheet.png"),
    video: require("@/assets/browse/video.png"),
} satisfies Record<string, ImageSourcePropType>;

const extensionIcons: Record<string, keyof typeof icons> = {
    pdf: "pdf",
    xls: "spreadsheet", xlsx: "spreadsheet", csv: "spreadsheet", numbers: "spreadsheet", ods: "spreadsheet", tsv: "spreadsheet",
    ppt: "slides", pptx: "slides", key: "slides", odp: "slides",
    doc: "document", docx: "document", txt: "document", md: "document", rtf: "document", pages: "document", odt: "document",
    jpg: "image", jpeg: "image", png: "image", gif: "image", heic: "image", webp: "image", svg: "image", bmp: "image", tiff: "image",
    mp4: "video", mov: "video", mkv: "video", avi: "video", webm: "video", m4v: "video",
};

/** Picks an icon from the MIME type, falling back to the file extension. */
export function iconFor(type: "directory" | "file", name: string, mime?: string): ImageSourcePropType {
    if (type === "directory") return icons.folder;
    if (mime) {
        if (mime.startsWith("image/")) return icons.image;
        if (mime.startsWith("video/")) return icons.video;
        if (mime === "application/pdf") return icons.pdf;
        if (/spreadsheet|excel|csv/.test(mime)) return icons.spreadsheet;
        if (/presentation|powerpoint/.test(mime)) return icons.slides;
        if (mime.startsWith("text/") || /word|document|rtf/.test(mime)) return icons.document;
    }
    const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
    return icons[extensionIcons[ext] ?? "file"];
}

export function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    const units = ["KB", "MB", "GB", "TB"];
    let value = bytes / 1024;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
        value /= 1024;
        unit++;
    }
    return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

export function formatDate(ms: number): string {
    return new Date(ms).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

type Props = {
    name: string;
    type: "directory" | "file";
    size?: number;
    modifiedMs?: number;
    mime?: string;
    onPress?: () => void;
};

export default function FileEntry({ name, type, size, modifiedMs, mime, onPress }: Props) {
    const font_size = useFontSize();
    // e.g. "1.2 MB · Mar 4, 2026"; size/date are optional in the API.
    const details = [
        type === "file" && size !== undefined ? formatBytes(size) : null,
        modifiedMs !== undefined ? formatDate(modifiedMs) : null,
    ].filter(Boolean).join(" · ");

    return (
        <TouchableOpacity style={styles.container} onPress={onPress}>
            <Image source={iconFor(type, name, mime)} style={styles.icon} />
            <View style={styles.text}>
                <Text style={{ fontSize: font_size }} numberOfLines={1} ellipsizeMode="middle">{name}</Text>
                {details ? <Text style={styles.details}>{details}</Text> : null}
            </View>
            {type === "directory" && <Text style={[styles.chevron, { fontSize: font_size + 6 }]}>›</Text>}
        </TouchableOpacity>
    );
}

const styles = StyleSheet.create({
    container: {
        flexDirection: "row",
        alignItems: "center",
        gap: 12,
        paddingVertical: 10,
        paddingHorizontal: 16,
        borderBottomWidth: StyleSheet.hairlineWidth,
        borderBottomColor: "#C7C7CC",
    },
    icon: {
        width: 32,
        height: 32,
    },
    // flex: 1 so long names truncate instead of pushing the chevron off-screen.
    text: {
        flex: 1,
    },
    details: {
        color: "#6C6C70",
        marginTop: 2,
    },
    chevron: {
        color: "#C7C7CC",
    },
});
