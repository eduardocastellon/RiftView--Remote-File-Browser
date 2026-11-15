import { useRef, useState } from "react";
import {
  StyleSheet,
  Text,
  TextInput,
  View,
  type TextInputInstance,
} from "react-native";

type Props = {
  /** Called with the digits entered so far (empty boxes are skipped). */
  onChange?: (code: string) => void;
  /** Called once every box is filled. */
  onComplete?: (code: string) => void;
  length?: number;
};

/**
 * One-time-code style input: one TextInput per digit, so any box can be
 * tapped and edited. Typing advances to the next box, backspace on an empty
 * box moves back, and pasting / SMS autofill spreads the code across boxes.
 *
 * Each TextInput is kept empty with invisible text; the digit is drawn by a
 * Text overlay. That way a new digit replaces the old one instantly instead
 * of briefly rendering next to it in the native input.
 */
export function CodeInput({ onChange, onComplete, length = 6 }: Props) {
  const [digits, setDigits] = useState<string[]>(() => Array(length).fill(""));
  const [focusedIndex, setFocusedIndex] = useState<number | null>(null);
  const refs = useRef<(TextInputInstance | null)[]>([]);

  const commit = (next: string[]) => {
    setDigits(next);
    const code = next.join("");
    onChange?.(code);
    if (next.every((d) => d !== "")) onComplete?.(code);
  };

  const handleChange = (index: number, text: string) => {
    // The native input is always empty, so `text` is only what was just
    // typed (one digit) or pasted / autofilled (several).
    const clean = text.replace(/\D/g, "");
    if (clean === "") return;
    const next = [...digits];

    // Full code pasted / autofilled: start from the first box.
    const start = clean.length >= length ? 0 : index;
    const chars = clean.slice(0, length - start).split("");
    chars.forEach((c, k) => {
      next[start + k] = c;
    });
    commit(next);

    // Move straight to the next box; focusing another input directly keeps
    // the keyboard up. Only dismiss it once the last box has been filled.
    const nextIndex = start + chars.length;
    if (nextIndex < length) {
      refs.current[nextIndex]?.focus();
    } else {
      const firstEmpty = next.indexOf("");
      if (firstEmpty === -1) refs.current[index]?.blur();
      else refs.current[firstEmpty]?.focus(); // last box filled but gaps remain
    }
  };

  const handleKeyPress = (index: number, key: string) => {
    if (key !== "Backspace") return;
    const next = [...digits];
    if (digits[index] !== "") {
      // Filled box: clear it and stay here.
      next[index] = "";
      commit(next);
    } else if (index > 0) {
      // Empty box: clear the previous one and jump back to it.
      next[index - 1] = "";
      commit(next);
      refs.current[index - 1]?.focus();
    }
  };

  return (
    <View style={styles.row}>
      {digits.map((digit, i) => (
        <View
          key={i}
          style={[styles.box, focusedIndex === i && styles.boxActive]}
        >
          <Text style={styles.digit}>{digit}</Text>
          <TextInput
            ref={(r) => {
              refs.current[i] = r;
            }}
            style={styles.input}
            value=""
            onChangeText={(t) => handleChange(i, t)}
            onKeyPress={(e) => handleKeyPress(i, e.nativeEvent.key)}
            onFocus={() => setFocusedIndex(i)}
            onBlur={() => setFocusedIndex((f) => (f === i ? null : f))}
            maxLength={length}
            keyboardType="number-pad"
            textContentType="oneTimeCode"
            autoComplete="one-time-code"
            // Hide the long-press copy/paste/select menu.
            contextMenuHidden
            caretHidden
          />
        </View>
      ))}
    </View>
  );
}

const styles = StyleSheet.create({
  row: {
    flexDirection: "row",
    gap: 8,
  },
  box: {
    width: 44,
    height: 54,
    borderWidth: 1,
    borderColor: "#C7C7CC",
    borderRadius: 8,
    backgroundColor: "white",
    alignItems: "center",
    justifyContent: "center",
  },
  digit: {
    fontSize: 24,
    fontWeight: "600",
    color: "black",
  },
  input: {
    // Covers the box so it receives taps, but draws nothing itself.
    // (Not opacity: 0 — iOS ignores touches on fully transparent views.)
    ...StyleSheet.absoluteFill,
    color: "transparent",
    backgroundColor: "transparent",
  },
  boxActive: {
    borderColor: "#007AFF",
    borderWidth: 2,
  },
});
