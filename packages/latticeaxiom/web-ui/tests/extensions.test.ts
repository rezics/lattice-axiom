import assert from "node:assert/strict";
import test from "node:test";
import { registeredPanels } from "../src/extensions.ts";
import { registerExamplePanel } from "../examples/package-panel.ts";

test("importing an extension has no registration or world activation side effect", () => {
    assert.equal(registeredPanels().length, 0);
    // Registration stores a contribution; DOM and transport are needed only on mount.
    registerExamplePanel();
    assert.equal(registeredPanels().length, 1);
    assert.equal(registeredPanels()[0].id, "@example/inspector:summary");
    assert.throws(registerExamplePanel, /duplicate panel/);
});
