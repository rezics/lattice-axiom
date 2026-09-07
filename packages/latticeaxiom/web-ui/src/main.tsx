import {
    Component,
    useEffect,
    useRef,
    useState,
    useSyncExternalStore,
} from "react";
import type { ErrorInfo, ReactNode } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { act, bridge } from "./bridge";
import { Shell } from "./Shell";
import { Game } from "./Game";
import { Settings } from "./Settings";
import "./style.css";
import { registeredPanels } from "./extensions";
import type { Json } from "./types";
import { themeTokens } from "./theme";

function ExtensionPanels({ states }: { states?: Record<string, Json> }) {
    const [selected, setSelected] = useState<string | null>(null);
    const mount = useRef<HTMLDivElement>(null);
    const instance = useRef<ReturnType<
        ReturnType<typeof registeredPanels>[number]["mount"]
    > | null>(null);
    const panels = registeredPanels();
    useEffect(() => {
        const panel = panels.find((panel) => panel.id === selected);
        if (!panel || !mount.current) return;
        instance.current = panel.mount(mount.current, {
            client: bridge,
            state: states?.[panel.id],
        });
        return () => {
            instance.current?.dispose();
            instance.current = null;
        };
    }, [selected]);
    useEffect(() => {
        instance.current?.update(selected ? states?.[selected] : undefined);
    }, [states, selected]);
    if (!panels.length) return null;
    return (
        <section className="extension-panels">
            <nav aria-label="Package panels">
                {panels.map((panel) => (
                    <button
                        key={panel.id}
                        onClick={() =>
                            setSelected(selected === panel.id ? null : panel.id)
                        }
                    >
                        {panel.label}
                    </button>
                ))}
            </nav>
            {selected ? <div className="extension-body" ref={mount} /> : null}
        </section>
    );
}

function App() {
    const state = useSyncExternalStore(bridge.subscribe, bridge.getSnapshot);
    const [error, setError] = useState<string | null>(null);
    const focusOwner = useRef(false);
    useEffect(() => {
        document.documentElement.style.setProperty(
            "--ui-scale",
            String(state?.uiScale ?? 1),
        );
    }, [state?.uiScale]);
    useEffect(() => {
        const tokens = themeTokens(state?.theme);
        for (const [key, value] of tokens)
            document.documentElement.style.setProperty(key, value);
        return () => {
            for (const [key] of tokens)
                document.documentElement.style.removeProperty(key);
        };
    }, [JSON.stringify(state?.theme)]);
    useEffect(() => {
        setError(state?.error ?? null);
    }, [
        state?.error,
        state?.mode,
        state?.shell?.screen,
        state?.shell?.tree.children[0]?.id,
        state?.game?.modal,
        state?.game?.overlay,
        state?.settings?.open,
    ]);
    useEffect(() => {
        const fail = (event: Event) =>
            setError((event as CustomEvent<string>).detail);
        const off = bridge.onError(setError);
        window.addEventListener("lattice:error", fail);
        const focus = () => {
            const element = document.activeElement;
            const text =
                element instanceof HTMLInputElement ||
                element instanceof HTMLTextAreaElement ||
                (element instanceof HTMLElement && element.isContentEditable);
            if (focusOwner.current !== text) {
                focusOwner.current = text;
                act("ui.focus", { text });
            }
        };
        document.addEventListener("focusin", focus);
        document.addEventListener("focusout", focus);
        return () => {
            off();
            window.removeEventListener("lattice:error", fail);
            document.removeEventListener("focusin", focus);
            document.removeEventListener("focusout", focus);
        };
    }, []);
    useEffect(() => {
        const keys = (event: KeyboardEvent) => {
            if (
                event.isComposing ||
                event.repeat ||
                (event.target instanceof HTMLElement &&
                    event.target.dataset.keyCapture === "true")
            )
                return;
            const text =
                event.target instanceof HTMLInputElement ||
                event.target instanceof HTMLTextAreaElement ||
                (event.target instanceof HTMLElement &&
                    event.target.isContentEditable);
            const modifiers = [
                event.shiftKey ? "shift" : "",
                event.ctrlKey ? "control" : "",
                event.altKey ? "alt" : "",
                event.metaKey ? "super" : "",
            ].filter(Boolean);
            const matches =
                state?.shortcuts?.filter(
                    (shortcut) =>
                        shortcut.code === event.code &&
                        shortcut.modifiers.length === modifiers.length &&
                        shortcut.modifiers.every((modifier) =>
                            modifiers.includes(modifier),
                        ),
                ) ?? [];
            const shortcut =
                matches.find((shortcut) => shortcut.action === "back") ??
                matches[0];
            const conventionalEscape =
                event.key === "Escape" &&
                (state?.mode !== "game" ||
                    state.settings?.open ||
                    !state.shortcuts);
            const navigateBack =
                shortcut?.action === "back" ||
                shortcut?.action === "pause" ||
                conventionalEscape;
            if (navigateBack && (!text || event.key === "Escape")) {
                event.preventDefault();
                if (state?.settings?.open) act("settings.cancel", {});
                else if (
                    state?.mode === "game" &&
                    !state.game?.saving?.requested
                )
                    act("game.surface", {
                        action:
                            state.game?.modal === "none" &&
                            state.game.overlay === "none"
                                ? "pause"
                                : "back",
                    });
                else {
                    const back = state?.shell?.tree.children.find((n) =>
                        n.actions.includes("back"),
                    );
                    if (back)
                        act("shell.action", {
                            target: back.id,
                            action: "back",
                        });
                }
            } else if (!text && state?.mode === "game" && event.code === "F3") {
                event.preventDefault();
                act("debug.toggle", {});
            } else if (
                !text &&
                state?.mode === "game" &&
                shortcut &&
                !state.settings?.open &&
                !state.game?.saving?.requested
            ) {
                if (
                    shortcut.action === "toggle-inventory" ||
                    shortcut.action === "toggle-workbench"
                ) {
                    event.preventDefault();
                    act("game.surface", {
                        action:
                            shortcut.action === "toggle-inventory"
                                ? "inventory"
                                : "workbench",
                    });
                } else if (shortcut.action.startsWith("hotbar-slot-")) {
                    event.preventDefault();
                    act("inventory.select", {
                        slot:
                            Number(
                                shortcut.action.slice("hotbar-slot-".length),
                            ) - 1,
                    });
                }
            }
        };
        window.addEventListener("keydown", keys);
        return () => window.removeEventListener("keydown", keys);
    }, [
        state?.mode,
        state?.settings?.open,
        state?.shell?.tree,
        state?.shortcuts,
        state?.game?.saving?.requested,
        state?.game?.modal,
        state?.game?.overlay,
    ]);
    useEffect(() => {
        const dialog = document.querySelector<HTMLElement>('[role="dialog"]');
        const first = dialog?.querySelector<HTMLElement>(
            "button:not(:disabled),input:not(:disabled),select:not(:disabled)",
        );
        first?.focus();
    }, [
        state?.settings?.open,
        state?.game?.modal,
        state?.game?.overlay,
        state?.game?.saving?.requested,
    ]);
    useEffect(() => {
        const trap = (event: KeyboardEvent) => {
            if (event.key !== "Tab") return;
            const dialog =
                document.querySelector<HTMLElement>('[role="dialog"]');
            if (!dialog) return;
            const controls = [
                ...dialog.querySelectorAll<HTMLElement>(
                    'button:not(:disabled),input:not(:disabled),select:not(:disabled),[tabindex="0"]',
                ),
            ];
            const index = controls.indexOf(
                document.activeElement as HTMLElement,
            );
            if (event.shiftKey && index <= 0) {
                event.preventDefault();
                controls.at(-1)?.focus();
            } else if (
                !event.shiftKey &&
                (index === controls.length - 1 || index === -1)
            ) {
                event.preventDefault();
                controls[0]?.focus();
            }
        };
        document.addEventListener("keydown", trap);
        return () => document.removeEventListener("keydown", trap);
    }, []);
    return (
        <>
            {!state ? (
                <main className="connection">
                    <h1>Terrenia</h1>
                    <p>Connecting to your game…</p>
                    <div className="loading-track" />
                    <small>
                        Launch the game client to open this interface.
                    </small>
                </main>
            ) : (
                <>
                    {state.mode === "shell" && state.shell ? (
                        <Shell shell={state.shell} />
                    ) : null}
                    {state.mode === "game" && state.game ? (
                        <Game game={state.game} />
                    ) : null}
                    {state.settings?.open ? (
                        <Settings settings={state.settings} />
                    ) : null}
                    <ExtensionPanels states={state.extensions} />
                </>
            )}
            {error ? (
                <aside className="error-toast" role="alert">
                    <div>
                        <strong>Could not complete the action</strong>
                        <p>{error}</p>
                    </div>
                    <button
                        aria-label="Dismiss error"
                        onClick={() => setError(null)}
                    >
                        ×
                    </button>
                </aside>
            ) : null}
        </>
    );
}
class ErrorBoundary extends Component<
    { children: ReactNode },
    { failed: boolean }
> {
    state = { failed: false };
    static getDerivedStateFromError() {
        return { failed: true };
    }
    componentDidCatch(error: Error, info: ErrorInfo) {
        console.error("UI render failure", error.message, info.componentStack);
    }
    render() {
        return this.state.failed ? (
            <main className="connection">
                <h1>The interface needs to reload</h1>
                <p>Your world remains managed by the game.</p>
                <button onClick={() => window.location.reload()}>
                    Reload interface
                </button>
            </main>
        ) : (
            this.props.children
        );
    }
}
const receive = window.__latticeReceive!;
window.__latticeReceive = (message: unknown) => {
    if (
        message &&
        typeof message === "object" &&
        "type" in message &&
        message.type === "navigation" &&
        "action" in message &&
        typeof message.action === "string"
    ) {
        const action = message.action;
        const scope = document.querySelector('[role="dialog"]') ?? document;
        const elements = [
            ...scope.querySelectorAll<HTMLElement>(
                "button:not(:disabled),input:not(:disabled),select:not(:disabled)",
            ),
        ].filter((el) => el.getClientRects().length > 0);
        const index = elements.indexOf(document.activeElement as HTMLElement);
        if (action === "activate")
            (document.activeElement as HTMLElement)?.click();
        else if (action === "back")
            window.dispatchEvent(
                new KeyboardEvent("keydown", { key: "Escape" }),
            );
        else {
            const delta = [
                "nav-up",
                "nav-left",
                "nav-previous",
                "focus-previous",
            ].includes(action)
                ? -1
                : 1;
            elements[
                (index + delta + elements.length) % elements.length
            ]?.focus();
        }
    } else receive(message);
};
async function start() {
    if (
        import.meta.env.DEV &&
        new URLSearchParams(location.search).has("fixture")
    ) {
        const { installFixture } = await import("./fixture");
        installFixture();
    }
    const root: Root =
        import.meta.hot?.data.root ??
        createRoot(document.getElementById("root")!);
    if (import.meta.hot) import.meta.hot.data.root = root;
    root.render(
        <ErrorBoundary>
            <App />
        </ErrorBoundary>,
    );
    window.ipc?.postMessage(
        JSON.stringify({
            id: "ready",
            session: "",
            revision: 0,
            method: "ready",
            params: {},
        }),
    );
}
void start();
