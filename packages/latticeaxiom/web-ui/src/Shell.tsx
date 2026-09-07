import { useState } from "react";
import { act, send } from "./bridge";
import type { SemanticNode, Snapshot } from "./types";

const titleByRoute: Record<string, string> = {
    home: "Welcome back",
    worlds: "Your worlds",
    "new-world": "Create a world",
    trash: "Recently removed",
    loading: "Opening your world",
    packages: "Packages & profiles",
    diagnostics: "About this installation",
    quit: "Leave Terrenia?",
};
function action(node: SemanticNode) {
    return node.actions.includes("activate") ? "activate" : node.actions[0];
}
export function Shell({ shell }: { shell: NonNullable<Snapshot["shell"]> }) {
    const route = shell.tree.children[0]?.id.split("/")[0] || "home";
    const nodes = shell.tree.children;
    const home = route === "home";
    const title = titleByRoute[route] || shell.screen || "Terrenia";
    const back = nodes.find((n) => n.actions.includes("back"));
    const selectedProfile = nodes.find(
        (n) => n.id === "new-world/profile-status",
    )?.value;
    return (
        <main className="shell">
            <aside className="sidebar">
                <div className="wordmark">Terrenia</div>
                <nav aria-label="Main navigation">
                    {home ? (
                        nodes
                            .filter((n) =>
                                [
                                    "home/worlds",
                                    "home/packages-profiles",
                                    "home/diagnostics-about",
                                ].includes(n.id),
                            )
                            .map((n) => <NodeButton key={n.id} node={n} />)
                    ) : back ? (
                        <NodeButton node={back} />
                    ) : null}
                    <button
                        disabled={route === "loading"}
                        onClick={() => act("settings.open", {})}
                    >
                        Settings & mods
                    </button>
                </nav>
                <div className="sidebar-bottom">
                    {home
                        ? nodes
                              .filter((n) => n.id === "home/quit")
                              .map((n) => <NodeButton key={n.id} node={n} />)
                        : null}
                </div>
            </aside>
            <section className="shell-content">
                <header className="page-header">
                    <div>
                        <h1>{title}</h1>
                        <p>
                            {home
                                ? "Continue exploring, or start somewhere new."
                                : route === "worlds"
                                  ? "Open a world, or manage its saved history."
                                  : route === "new-world"
                                    ? "Name your world and choose its terrain."
                                    : ""}
                        </p>
                    </div>
                </header>
                {shell.message ? (
                    <p className="notice" role="status">
                        {shell.message}
                    </p>
                ) : null}
                <div
                    className={`semantic-content ${home ? "home-content" : ""}`}
                >
                    {nodes
                        .filter(
                            (n) =>
                                n !== back &&
                                (!home ||
                                    ![
                                        "home/worlds",
                                        "home/packages-profiles",
                                        "home/diagnostics-about",
                                        "home/quit",
                                    ].includes(n.id)),
                        )
                        .map((n) => (
                            <Semantic
                                key={n.id}
                                node={n}
                                worldName={shell.worldName}
                                selectedProfile={selectedProfile}
                            />
                        ))}
                    {route === "worlds" &&
                    !nodes.some(
                        (n) =>
                            n.role === "list-item" || n.id.startsWith("world:"),
                    ) ? (
                        <div className="empty">
                            <h2>No worlds yet</h2>
                            <p>Go back and create your first world.</p>
                        </div>
                    ) : null}
                </div>
            </section>
        </main>
    );
}
function NodeButton({
    node,
    selected,
}: {
    node: SemanticNode;
    selected?: boolean;
}) {
    const [busy, setBusy] = useState(false);
    const primary =
        [
            "home/continue",
            "home/new-world",
            "new-world/quick-create",
            "quit/confirm",
        ].includes(node.id) || node.id.endsWith("/play");
    return (
        <button
            className={
                primary
                    ? "primary"
                    : selected === undefined
                      ? ""
                      : `profile-option ${selected ? "selected" : ""}`
            }
            disabled={node.state.disabled || busy}
            aria-busy={busy}
            aria-pressed={selected}
            title={node.description || undefined}
            onClick={async () => {
                setBusy(true);
                try {
                    await send("shell.action", {
                        target: node.id,
                        action: action(node),
                    });
                } catch {
                } finally {
                    setBusy(false);
                }
            }}
        >
            {busy ? "Please wait…" : node.name}
        </button>
    );
}
function Semantic({
    node,
    worldName,
    selectedProfile,
}: {
    node: SemanticNode;
    worldName: string;
    selectedProfile?: string | null;
}) {
    if (node.role === "text-input")
        return (
            <label className="world-name field-label">
                {node.name}
                <input
                    aria-label={node.name}
                    defaultValue={node.value ?? worldName}
                    maxLength={128}
                    onChange={(e) =>
                        act("shell.name", { value: e.target.value })
                    }
                />
                <small>{node.description}</small>
            </label>
        );
    if (node.children.length)
        return (
            <article className="world-row">
                <div className="world-description">
                    <h2>{node.name}</h2>
                    {node.value ? (
                        <p className="muted mono">{node.value}</p>
                    ) : null}
                    {node.role === "alert" && node.description ? (
                        <p className="notice" role="alert">
                            {node.description}
                        </p>
                    ) : null}
                </div>
                <div className="world-actions">
                    {node.children.map((child) => (
                        <Semantic
                            key={child.id}
                            node={child}
                            worldName={worldName}
                            selectedProfile={selectedProfile}
                        />
                    ))}
                </div>
            </article>
        );
    if (node.role === "button" || node.actions.length)
        return (
            <NodeButton
                node={node}
                selected={
                    node.id.startsWith("new-world/profile/")
                        ? node.name === selectedProfile
                        : undefined
                }
            />
        );
    return (
        <div
            className={node.role === "alert" ? "notice" : "semantic-status"}
            role={node.role === "alert" ? "alert" : "status"}
        >
            <strong>{node.name}</strong>
            {node.value ? <p>{node.value}</p> : null}
            {node.description ? (
                <p className="muted">{node.description}</p>
            ) : null}
            {node.state.busy ? <div className="loading-track" /> : null}
        </div>
    );
}
