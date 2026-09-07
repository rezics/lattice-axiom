import { registerPanel } from "../src/extensions.ts";

/** Called explicitly by a product's selected UI entrypoint, never at import time. */
export function registerExamplePanel() {
    registerPanel({
        id: "@example/inspector:summary",
        label: "Example inspector",
        mount(element, { client, state }) {
            const heading = document.createElement("h2");
            heading.textContent = "Package inspector";
            const output = document.createElement("pre");
            const refresh = document.createElement("button");
            refresh.textContent = "Refresh package summary";
            const update = (value: unknown) => {
                output.textContent = JSON.stringify(value ?? null, null, 2);
            };
            let disposed = false;
            const load = async () => {
                refresh.disabled = true;
                try {
                    const result = await client.invoke(
                        "@example/inspector:summary",
                        {},
                    );
                    if (!disposed) update(result);
                } catch (error) {
                    if (!disposed)
                        output.textContent =
                            error instanceof Error
                                ? error.message
                                : String(error);
                } finally {
                    if (!disposed) refresh.disabled = false;
                }
            };
            refresh.addEventListener("click", load);
            element.replaceChildren(heading, output, refresh);
            update(state);
            return {
                update,
                dispose() {
                    disposed = true;
                    refresh.removeEventListener("click", load);
                    element.replaceChildren();
                },
            };
        },
    });
}
