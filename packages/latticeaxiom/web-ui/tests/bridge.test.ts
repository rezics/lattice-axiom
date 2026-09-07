import assert from "node:assert/strict";
import test from "node:test";
import { ClientBridge } from "../src/bridge.ts";
import type { Request, Snapshot } from "../src/types.ts";
const snapshot = (session = "one", revision = 1): Snapshot => ({
    protocol: 1,
    session,
    revision,
    mode: "shell",
});

test("requests carry authoritative session/revision and resolve only matching responses", async () => {
    const sent: Request[] = [];
    const client = new ClientBridge((request) => sent.push(request));
    client.receive({ type: "snapshot", state: snapshot("session-a", 7) });
    const request = client.request("inventory.move", { from: 1, to: 2 });
    assert.deepEqual(sent[0], {
        id: "web-1",
        session: "session-a",
        revision: 7,
        method: "inventory.move",
        params: { from: 1, to: 2 },
    });
    client.receive({ type: "result", id: sent[0].id, ok: true });
    await request;
});
test("older same-session snapshots are discarded and new sessions invalidate pending edits", async () => {
    const client = new ClientBridge(() => {});
    client.receive({ type: "snapshot", state: snapshot("one", 3) });
    client.receive({ type: "snapshot", state: snapshot("one", 2) });
    assert.equal(client.getSnapshot()?.revision, 3);
    const request = client.request("settings.set", {
        id: "package:setting",
        value: true,
    });
    const rejected = assert.rejects(request, /session changed/);
    client.receive({ type: "snapshot", state: snapshot("two", 1) });
    await rejected;
    assert.equal(client.getSnapshot()?.session, "two");
});
test("native rejection is surfaced without optimistic inventory mutation", async () => {
    const sent: Request[] = [];
    const client = new ClientBridge((request) => sent.push(request));
    client.receive({ type: "snapshot", state: snapshot() });
    const request = client.request("inventory.move", { from: 2, to: 4 });
    const rejected = assert.rejects(request, /revision conflict/);
    client.receive({
        type: "result",
        id: sent[0].id,
        ok: false,
        error: "revision conflict",
    });
    await rejected;
    assert.equal(client.getSnapshot()?.revision, 1);
});
test("package endpoints can return typed JSON through the same boundary", async () => {
    const sent: Request[] = [];
    const client = new ClientBridge((request) => sent.push(request));
    client.receive({ type: "snapshot", state: snapshot() });
    const request = client.invoke("@example/inspector:summary", {});
    client.receive({
        type: "result",
        id: sent[0].id,
        ok: true,
        value: { name: "Selected package", count: 4 },
    });
    assert.deepEqual(await request, { name: "Selected package", count: 4 });
});
