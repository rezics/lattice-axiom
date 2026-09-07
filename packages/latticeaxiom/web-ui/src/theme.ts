/** Only plain color values from the presentation provider become CSS tokens. */
const TOKEN_MAP: Readonly<Record<string, string>> = {
    "--bg": "--background",
    "--background": "--background",
    "--surface": "--surface",
    "--surface-hover": "--surface-hover",
    "--accent": "--accent",
    "--line": "--line",
    "--muted": "--muted",
    "--text": "--text",
    "--danger": "--danger",
};
const COLOR = /^(?:#[\da-f]{3,8}|(?:rgb|rgba|hsl|hsla)\([\d\s.,%/+\-]+\))$/i;
export function themeTokens(
    theme: Record<string, string> | undefined,
): [string, string][] {
    return Object.entries(theme ?? {}).flatMap(([key, value]) => {
        const token = TOKEN_MAP[key.startsWith("--") ? key : `--${key}`];
        return token &&
            typeof value === "string" &&
            value.length <= 80 &&
            COLOR.test(value)
            ? [[token, value] as [string, string]]
            : [];
    });
}
