/** Explicit developer-only browser fixture. Vite removes this module from release builds. */
import { bridge } from "./bridge";
import type { Request, SemanticNode, Snapshot } from "./types";
const button = (id: string, name: string): SemanticNode => ({
    id,
    name,
    role: "button",
    value: null,
    description: null,
    state: {
        disabled: false,
        focused: false,
        focusable: true,
        expanded: null,
        busy: false,
    },
    actions: ["activate"],
    children: [],
});
export function installFixture() {
    document.body.style.background = "#223538";
    const page = new URLSearchParams(location.search).get("fixture");
    const shell = {
        tree: {
            ...button("shell", "Terrenia"),
            role: "application",
            children: [
                button("home/worlds", "Worlds"),
                button("home/new-world", "New World"),
                button("home/continue", "Continue — Browser fixture"),
                button("home/packages-profiles", "Packages & profiles"),
                button("home/diagnostics-about", "Diagnostics & about"),
                button("home/quit", "Quit"),
            ],
        },
        worldName: "Browser fixture",
        message: "Browser development fixture — no saved world is connected.",
    };
    const state: Snapshot = {
        protocol: 1,
        uiScale: Number(new URLSearchParams(location.search).get("scale") || 1),
        session: "explicit-browser-fixture",
        revision: 1,
        mode: page === "shell" || page === "loading" ? "shell" : "game",
        shell,
        game: {
            overlay:
                page === "inventory" || page === "catalog"
                    ? "inventory"
                    : "none",
            modal: "none",
            creative: true,
            hotbar: 0,
            slots: Array.from({ length: 36 }, (_, index) => ({
                index,
                item: index < 13 ? `fixture:block-${index}` : null,
                name: ["Grass", "Stone", "Sand", "Oak wood"][index % 4],
                quantity: index < 13 ? 64 : 0,
                color: ["#6b9470", "#869596", "#c6b783", "#997452"][index % 4],
            })),
            items: [
                "Grass",
                "Stone",
                "Sand",
                "Oak wood",
                "Planks",
                "Iron ore",
            ].map((name, i) => ({
                id: `fixture:item-${i}`,
                name,
                category: i < 3 ? "Blocks" : "Materials",
                color: ["#6b9470", "#869596", "#c6b783"][i % 3],
            })),
            recipes: [
                { id: "fixture:planks", name: "Oak planks", craftable: true },
                { id: "fixture:torch", name: "Torch", craftable: false },
            ],
            target:
                page === "empty"
                    ? null
                    : {
                          name: "Grass block",
                          id: "fixture:grass",
                          owner: "@fixture/blocks",
                          position: "0, 64, 0",
                      },
            debug: {
                visible: page === "debug",
                sections: [
                    {
                        title: "World",
                        rows: [
                            { label: "Name", value: "Browser fixture" },
                            { label: "Mode", value: "Fixture only" },
                        ],
                    },
                    {
                        title: "Player",
                        rows: [
                            { label: "Position", value: "0.00 / 64.00 / 0.00" },
                        ],
                    },
                    {
                        title: "Rendering",
                        rows: [
                            {
                                label: "GPU metrics",
                                value: "Unavailable in browser fixture",
                            },
                        ],
                    },
                ],
            },
        },
        settings: {
            open: page === "settings",
            dirty: false,
            message: null,
            owners: [
                { id: "@fixture/client", name: "Client preferences" },
                { id: "@fixture/inspect", name: "Target information" },
                {
                    id: "@fixture/no-settings",
                    name: "No configurable settings",
                },
            ],
            rows: [
                {
                    id: "fixture:inspect/enabled",
                    owner: "@fixture/inspect",
                    category: "hud",
                    label: "Show target information",
                    description:
                        "Display information when the crosshair points at a block.",
                    control: "toggle",
                    value: true,
                    defaultValue: true,
                    editable: true,
                    reason: null,
                    impact: "immediate",
                    schema: { type: "bool" },
                },
                {
                    id: "fixture:inspect/anchor",
                    owner: "@fixture/inspect",
                    category: "hud",
                    label: "Information position",
                    description:
                        "Choose where target information appears on screen.",
                    control: "enum-cycle",
                    value: "top-center",
                    defaultValue: "top-center",
                    editable: true,
                    reason: null,
                    impact: "immediate",
                    schema: {
                        type: "enum",
                        values: ["top-center", "top-left", "top-right"],
                    },
                },
                {
                    id: "fixture:client/distance",
                    owner: "@fixture/client",
                    category: "graphics",
                    label: "Render distance",
                    description: "Maximum terrain distance in chunks.",
                    control: "integer-slider",
                    value: 12,
                    defaultValue: 12,
                    editable: true,
                    reason: null,
                    impact: "immediate",
                    schema: { type: "integer", min: 4, max: 32, step: 1 },
                },
            ],
        },
    };
    if (page === "loading")
        state.shell!.tree.children = [
            {
                ...button("loading/status", "Preparing world"),
                role: "status",
                actions: [],
                state: { ...button("", "").state, busy: true },
                description:
                    "Loading the selected world and preparing its resources.",
            },
            {
                ...button("loading/cancel", "Cancel loading"),
                actions: ["cancel-loading"],
            },
        ];
    if (page === "saving" || page === "save-error")
        state.game!.saving = {
            requested: true,
            saved: false,
            error:
                page === "save-error"
                    ? "Fixture: the destination is temporarily unavailable."
                    : null,
        };
    state.settings!.bindings = [
        {
            id: "fixture:inventory",
            label: "Open inventory",
            context: "gameplay",
            code: "KeyE",
            modifiers: [],
            editable: true,
        },
    ];
    state.shortcuts = [
        { code: "KeyI", modifiers: [], action: "toggle-inventory" },
        { code: "Escape", modifiers: [], action: "back" },
    ];
    const fullCatalog = Array.from({ length: 205 }, (_, index) => ({
        id: `fixture:catalog-${index}`,
        name: `Block ${index + 1}`,
        category: index % 2 ? "Stone" : "Wood",
        color: "#78958b",
    }));
    if (page === "catalog") {
        state.game!.catalog = {
            page: 0,
            pageSize: 96,
            total: 205,
            pages: 3,
            query: "",
            category: "all",
            categories: [
                { id: "wood", name: "Wood" },
                { id: "stone", name: "Stone" },
            ],
            recipePage: 0,
            recipePages: 1,
            recipeTotal: 2,
        };
        state.game!.items = fullCatalog.slice(0, 96);
    }
    const publish = () => {
        state.revision += 1;
        bridge.receive({ type: "snapshot", state: structuredClone(state) });
    };
    window.ipc = {
        postMessage(raw) {
            const request = JSON.parse(raw) as Request;
            const params = request.params;
            if (request.method === "catalog.query" && state.game!.catalog) {
                const catalog = state.game!.catalog;
                if (typeof params.query === "string")
                    catalog.query = params.query;
                if (typeof params.category === "string")
                    catalog.category = params.category;
                const filtered = fullCatalog.filter(
                    (item) =>
                        item.name
                            .toLowerCase()
                            .includes(catalog.query.toLowerCase()) &&
                        (catalog.category === "all" ||
                            item.category.toLowerCase() === catalog.category),
                );
                catalog.total = filtered.length;
                catalog.pages = Math.max(
                    1,
                    Math.ceil(filtered.length / catalog.pageSize),
                );
                catalog.page = Math.min(
                    Number(params.page ?? catalog.page),
                    catalog.pages - 1,
                );
                state.game!.items = filtered.slice(
                    catalog.page * catalog.pageSize,
                    (catalog.page + 1) * catalog.pageSize,
                );
            }
            if (request.method === "settings.open") state.settings!.open = true;
            if (request.method === "settings.cancel") {
                state.settings!.open = false;
                state.settings!.dirty = false;
            }
            if (request.method === "settings.bind") {
                const row = state.settings!.bindings!.find(
                    (row) => row.id === params.id,
                );
                if (row) {
                    row.code = String(params.code);
                    row.modifiers = params.modifiers as string[];
                    state.settings!.dirty = true;
                }
            }
            if (request.method === "settings.unbind") {
                const row = state.settings!.bindings!.find(
                    (row) => row.id === params.id,
                );
                if (row) {
                    row.code = null;
                    row.modifiers = [];
                    state.settings!.dirty = true;
                }
            }
            if (request.method === "settings.apply")
                state.settings!.dirty = false;
            if (request.method === "settings.set") {
                const row = state.settings!.rows.find(
                    (r) => r.id === params.id,
                );
                if (row) {
                    row.value = params.value;
                    state.settings!.dirty = true;
                }
            }
            if (request.method === "settings.reset") {
                for (const row of state.settings!.rows)
                    if (
                        (!params.owner || row.owner === params.owner) &&
                        (!params.id || row.id === params.id)
                    )
                        row.value = row.defaultValue;
                state.settings!.dirty = true;
            }
            if (request.method === "game.surface") {
                const game = state.game!;
                if (params.action === "inventory")
                    game.overlay =
                        game.overlay === "none" ? "inventory" : "none";
                if (params.action === "pause") game.modal = "pause";
                if (params.action === "back" || params.action === "resume") {
                    game.overlay = "none";
                    game.modal = "none";
                }
            }
            if (request.method === "inventory.move") {
                const slots = state.game!.slots;
                const from = Number(params.from),
                    to = Number(params.to);
                [slots[from], slots[to]] = [
                    { ...slots[to], index: from },
                    { ...slots[from], index: to },
                ];
            }
            if (request.method === "inventory.select")
                state.game!.hotbar = Number(params.slot);
            if (request.method === "debug.toggle")
                state.game!.debug.visible = !state.game!.debug.visible;
            if (
                request.method === "shell.action" &&
                params.target === "home/worlds"
            )
                state.shell!.tree.children = [
                    {
                        ...button("worlds/back", "Back"),
                        actions: ["activate", "back"],
                    },
                    button("worlds/trash", "Trash"),
                    {
                        ...button("world:fixture", "Browser fixture"),
                        role: "list-item",
                        children: [
                            button("world:fixture/play", "Open"),
                            button("world:fixture/details", "Details"),
                        ],
                    },
                ];
            publish();
            if (request.method !== "ready")
                bridge.receive({ type: "result", id: request.id, ok: true });
        },
    };
    publish();
}
