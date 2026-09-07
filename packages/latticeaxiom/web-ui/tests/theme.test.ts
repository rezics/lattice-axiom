import assert from "node:assert/strict";
import test from "node:test";
import { themeTokens } from "../src/theme.ts";
test("theme only applies known presentation colors without URLs or CSS declarations", () => {
    assert.deepEqual(
        themeTokens({
            "--bg": "#123456",
            accent: "rgb(12 34 56)",
            unknown: "#fff",
            "--text": "url(https://example.com)",
            "--surface": "#fff;display:none",
        }),
        [
            ["--background", "#123456"],
            ["--accent", "rgb(12 34 56)"],
        ],
    );
});
