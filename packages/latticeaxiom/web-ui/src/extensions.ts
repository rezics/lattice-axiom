import type { ClientBridge } from "./bridge.ts";
import type { Json } from "./types.ts";

export interface PanelContribution {
    /** Stable package-qualified panel identity. */
    id: string;
    label: string;
    mount(
        element: HTMLElement,
        context: { client: ClientBridge; state: Json | undefined },
    ): { update(state: Json | undefined): void; dispose(): void };
}
const panels = new Map<string, PanelContribution>();
/** A product explicitly registers selected trusted code packages before mounting its app. */
export function registerPanel(panel: PanelContribution) {
    if (!panel.id.includes("/") || panels.has(panel.id))
        throw new Error(`Invalid or duplicate panel: ${panel.id}`);
    panels.set(panel.id, panel);
}
export function registeredPanels(): readonly PanelContribution[] {
    return [...panels.values()];
}
