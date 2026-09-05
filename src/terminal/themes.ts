import type { ITheme } from "@xterm/xterm";
import type { ResolvedAppearance } from "../theme";

const darkTheme: ITheme = {
  background: "#11151a",
  foreground: "#e8edf2",
  cursor: "#75b8ff",
  cursorAccent: "#11151a",
  selectionBackground: "#75b8ff55",
  selectionInactiveBackground: "#596b7f55",
  black: "#171c22",
  red: "#ff7f8c",
  green: "#77dba5",
  yellow: "#f0c36a",
  blue: "#75b8ff",
  magenta: "#c995f7",
  cyan: "#67ced2",
  white: "#e8edf2",
  brightBlack: "#8a96a3",
  brightRed: "#ff8991",
  brightGreen: "#89e5a8",
  brightYellow: "#f3d98d",
  brightBlue: "#98beff",
  brightMagenta: "#d9b2fa",
  brightCyan: "#8fe1e3",
  brightWhite: "#ffffff",
};

const lightTheme: ITheme = {
  background: "#ffffff",
  foreground: "#17212b",
  cursor: "#246fce",
  cursorAccent: "#ffffff",
  selectionBackground: "#246fce44",
  selectionInactiveBackground: "#69788744",
  black: "#17212b",
  red: "#c43d4c",
  green: "#1f8d56",
  yellow: "#a06a00",
  blue: "#246fce",
  magenta: "#8050ad",
  cyan: "#18777c",
  white: "#d8e0e8",
  brightBlack: "#697887",
  brightRed: "#dc4d58",
  brightGreen: "#36945a",
  brightYellow: "#ad821f",
  brightBlue: "#397fd3",
  brightMagenta: "#9767c3",
  brightCyan: "#288e93",
  brightWhite: "#ffffff",
};

export function terminalTheme(appearance: ResolvedAppearance): ITheme {
  return appearance === "dark" ? { ...darkTheme } : { ...lightTheme };
}
