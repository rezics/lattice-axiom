import type { Envelope, Json, Methods, Request, Snapshot } from "./types.ts";

declare global {
    interface Window {
        ipc?: { postMessage(message: string): void };
        __latticeReceive?: (message: unknown) => void;
    }
}

/** Framework independent bounded client. Registration/import never activates a world. */
export class ClientBridge {
    private snapshot: Snapshot | null = null;
    private listeners = new Set<() => void>();
    private errors = new Set<(message: string) => void>();
    private pending = new Map<
        string,
        {
            resolve: (value: Json) => void;
            reject: (error: Error) => void;
            timer: ReturnType<typeof setTimeout>;
        }
    >();
    private sequence = 0;
    private readonly transport: (request: Request) => void;
    constructor(transport: (request: Request) => void) {
        this.transport = transport;
    }
    getSnapshot = () => this.snapshot;
    subscribe = (listener: () => void) => {
        this.listeners.add(listener);
        return () => {
            this.listeners.delete(listener);
        };
    };
    onError(listener: (message: string) => void) {
        this.errors.add(listener);
        return () => {
            this.errors.delete(listener);
        };
    }
    private fail(message: string) {
        for (const listener of this.errors) listener(message);
    }
    receive = (input: unknown) => {
        if (!input || typeof input !== "object") return;
        const message = input as Envelope;
        if (message.type === "snapshot") {
            const state = message.state;
            if (
                state?.protocol !== 1 ||
                typeof state.session !== "string" ||
                !Number.isSafeInteger(state.revision) ||
                !["shell", "game"].includes(state.mode)
            ) {
                this.fail(
                    "The client UI protocol is incompatible. Restart the game.",
                );
                return;
            }
            if (
                this.snapshot?.session === state.session &&
                state.revision < this.snapshot.revision
            )
                return;
            if (this.snapshot && this.snapshot.session !== state.session) {
                for (const request of this.pending.values()) {
                    clearTimeout(request.timer);
                    request.reject(new Error("The game session changed."));
                }
                this.pending.clear();
            }
            this.snapshot = state;
            for (const listener of this.listeners) listener();
        } else if (message.type === "result") {
            const pending = this.pending.get(message.id);
            if (!pending) return;
            clearTimeout(pending.timer);
            this.pending.delete(message.id);
            if (message.ok) pending.resolve(message.value ?? null);
            else
                pending.reject(
                    new Error(
                        message.error ||
                            "The operation could not be completed.",
                    ),
                );
        }
    };
    request<M extends keyof Methods>(
        method: M,
        params: Methods[M],
    ): Promise<void> {
        return this.invoke(method, params as Record<string, Json>).then(
            () => {},
        );
    }
    /** Extension calls use package namespaced methods and the same validation boundary. */
    invoke(method: string, params: Record<string, Json>): Promise<Json> {
        if (!this.snapshot)
            return Promise.reject(
                new Error("Waiting for the game connection."),
            );
        if (this.pending.size >= 64)
            return Promise.reject(
                new Error("Too many pending operations. Please wait."),
            );
        const id = `web-${++this.sequence}`;
        const request = {
            id,
            session: this.snapshot.session,
            revision: this.snapshot.revision,
            method,
            params,
        };
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => {
                this.pending.delete(id);
                reject(
                    new Error("The game did not respond. Please try again."),
                );
            }, 15000);
            this.pending.set(id, { resolve, reject, timer });
            try {
                this.transport(request);
            } catch (error) {
                clearTimeout(timer);
                this.pending.delete(id);
                reject(error);
            }
        });
    }
}

export const bridge = new ClientBridge((request) => {
    if (!window.ipc)
        throw new Error("The native game connection is unavailable.");
    window.ipc.postMessage(JSON.stringify(request));
});
if (typeof window !== "undefined") window.__latticeReceive = bridge.receive;

export function send<M extends keyof Methods>(method: M, params: Methods[M]) {
    return bridge.request(method, params).catch((error: unknown) => {
        window.dispatchEvent(
            new CustomEvent("lattice:error", {
                detail: error instanceof Error ? error.message : String(error),
            }),
        );
        throw error;
    });
}
/** Fire-and-display event handler helper; promise rejection is always observed. */
export function act<M extends keyof Methods>(method: M, params: Methods[M]) {
    void send(method, params).catch(() => {});
}
