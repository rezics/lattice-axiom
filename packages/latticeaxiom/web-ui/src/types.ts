/** Version one local client boundary. World and entity identities remain strings. */
export type Json =
    null | boolean | number | string | Json[] | { [key: string]: Json };
export interface SemanticNode {
    id: string;
    role: string;
    name: string;
    value: string | null;
    description: string | null;
    state: {
        focusable: boolean;
        focused: boolean;
        disabled: boolean;
        expanded: boolean | null;
        busy: boolean;
    };
    actions: string[];
    children: SemanticNode[];
}
export interface Slot {
    index: number;
    item: string | null;
    name: string;
    quantity: number;
    color: string;
}
export interface Item {
    id: string;
    name: string;
    category: string;
    color: string;
}
export interface GameState {
    overlay: "none" | "inventory" | "workbench";
    modal: "none" | "pause" | "settings" | "confirm-save-quit";
    creative: boolean;
    hotbar: number;
    slots: Slot[];
    items: Item[];
    recipes: { id: string; name: string; craftable: boolean }[];
    target: null | {
        name: string;
        id: string;
        owner: string;
        position: string;
        anchor?: "top-center" | "top-left" | "top-right";
        harvestTool?: string;
        hardnessTicks?: number;
    };
    saving?: {
        requested: boolean;
        saved: boolean;
        error?: string | null;
        returnToShell?: boolean;
        checkpoints?: number;
    };
    hint?: string;
    tool?: { name: string; durability: number; maxDurability: number };
    toolDurability?: number | null;
    catalog?: {
        page: number;
        pageSize: number;
        total: number;
        pages: number;
        query: string;
        category: string;
        categories: { id: string; name: string }[];
        recipePage: number;
        recipePages: number;
        recipeTotal: number;
        error?: string | null;
    };
    debug: {
        visible: boolean;
        sections: { title: string; rows: { label: string; value: string }[] }[];
    };
}
export interface ValueSchema {
    type: string;
    min?: number | null;
    max?: number | null;
    step?: number | null;
    min_length?: number | null;
    max_length?: number | null;
    values?: string[];
}
export interface SettingRow {
    id: string;
    owner: string;
    category: string;
    label: string;
    description: string;
    control: string;
    value: Json;
    defaultValue: Json;
    editable: boolean;
    reason: string | null;
    impact: string;
    schema: ValueSchema;
    visible?: boolean;
}
export interface BindingRow {
    id: string;
    label: string;
    context: string;
    description?: string;
    display?: string;
    code: string | null;
    modifiers: string[];
    editable: boolean;
}
export interface SettingsState {
    open: boolean;
    dirty: boolean;
    message: string | null;
    rows: SettingRow[];
    owners?: { id: string; name: string }[];
    bindings?: BindingRow[];
}
export interface Snapshot {
    protocol: 1;
    session: string;
    revision: number;
    mode: "shell" | "game";
    shell?: {
        tree: SemanticNode;
        worldName: string;
        message?: string;
        screen?: string;
    };
    game?: GameState;
    settings?: SettingsState;
    extensions?: Record<string, Json>;
    error?: string | null;
    uiScale?: number;
    shortcuts?: { code: string; modifiers: string[]; action: string }[];
    theme?: Record<string, string>;
}
export interface Request {
    id: string;
    session: string;
    revision: number;
    method: string;
    params: Record<string, Json>;
}
export type Envelope =
    | { type: "snapshot"; state: Snapshot }
    | { type: "result"; id: string; ok: boolean; error?: string; value?: Json };
export interface Methods {
    "shell.action": { target: string; action: string };
    "shell.name": { value: string };
    "game.surface": {
        action:
            | "inventory"
            | "workbench"
            | "pause"
            | "back"
            | "settings"
            | "resume";
    };
    "inventory.move": { from: number; to: number };
    "inventory.select": { slot: number };
    "inventory.pick": { item: string };
    "recipe.craft": { recipe: string };
    "game.save": {};
    "game.exit": {};
    "debug.toggle": {};
    "settings.set": { id: string; value: Json };
    "settings.apply": {};
    "settings.cancel": {};
    "settings.reset": { owner?: string; id?: string };
    "settings.open": {};
    "ui.focus": { text: boolean };
    "app.quit": {};
    "settings.bind": { id: string; code: string; modifiers: string[] };
    "settings.unbind": { id: string };
    "settings.resetBinding": { id: string };
    "catalog.query": {
        query?: string;
        category?: string;
        page?: number;
        recipePage?: number;
    };
}
