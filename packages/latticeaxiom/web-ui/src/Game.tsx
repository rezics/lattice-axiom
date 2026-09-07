import { memo, useDeferredValue, useEffect, useState } from "react";
import { act, send } from "./bridge";
import type { GameState, Item, Slot } from "./types";

export function Game({ game }: { game: GameState }) {
    return (
        <main className="game" aria-label="Game interface">
            {game.target && game.modal === "none" && game.overlay === "none" ? (
                <aside
                    className={`target-info anchor-${game.target.anchor ?? "top-center"}`}
                >
                    <strong>{game.target.name}</strong>
                    <span>{game.target.owner}</span>
                    {game.target.harvestTool ? (
                        <span>Tool: {game.target.harvestTool}</span>
                    ) : null}
                </aside>
            ) : null}
            {game.hint && game.modal === "none" && game.overlay === "none" ? (
                <p className="game-hint" role="status">
                    {game.hint}
                </p>
            ) : null}
            {game.tool && game.modal === "none" && game.overlay === "none" ? (
                <div className="tool-durability">
                    <span>{game.tool.name}</span>
                    <meter
                        min={0}
                        max={game.tool.maxDurability}
                        value={game.tool.durability}
                        aria-label={`${game.tool.name} durability`}
                    />
                </div>
            ) : null}
            {!game.tool &&
            game.toolDurability != null &&
            game.modal === "none" &&
            game.overlay === "none" ? (
                <div className="tool-durability">
                    Tool durability: {game.toolDurability}
                </div>
            ) : null}
            {game.debug.visible ? (
                <aside className="debug-panel" aria-label="Debug information">
                    <header>
                        <h2>Diagnostics</h2>
                        <kbd>F3</kbd>
                    </header>
                    {game.debug.sections.map((section) => (
                        <section key={section.title}>
                            <h3>{section.title}</h3>
                            <dl>
                                {section.rows.map((row) => (
                                    <div key={row.label}>
                                        <dt>{row.label}</dt>
                                        <dd>{row.value}</dd>
                                    </div>
                                ))}
                            </dl>
                        </section>
                    ))}
                </aside>
            ) : null}
            {game.modal === "none" && game.overlay === "none" ? (
                <div className="hotbar" aria-label="Quick access">
                    {game.slots.slice(0, 9).map((slot) => (
                        <SlotButton
                            key={slot.index}
                            slot={slot}
                            active={slot.index === game.hotbar}
                            onClick={() =>
                                act("inventory.select", { slot: slot.index })
                            }
                        />
                    ))}
                </div>
            ) : null}
            {game.overlay !== "none" && game.modal === "none" ? (
                <Inventory game={game} />
            ) : null}
            {!game.saving?.requested &&
            (game.modal === "pause" || game.modal === "confirm-save-quit") ? (
                <div className="scrim">
                    <section
                        className="pause-panel"
                        aria-label="Pause menu"
                        role="dialog"
                        aria-modal="true"
                    >
                        <h1>
                            {game.modal === "confirm-save-quit"
                                ? "Return to your worlds?"
                                : "Paused"}
                        </h1>
                        <p className="muted">
                            {game.modal === "confirm-save-quit"
                                ? "Save your progress before leaving this world."
                                : "Take your time. Your world can wait."}
                        </p>
                        <div className="pause-actions">
                            <button
                                className="primary"
                                onClick={() =>
                                    act("game.surface", { action: "resume" })
                                }
                            >
                                Resume game
                            </button>
                            <button onClick={() => act("settings.open", {})}>
                                Settings & mods
                            </button>
                            <AsyncButton method="game.save">
                                Save world
                            </AsyncButton>
                            <AsyncButton method="game.exit">
                                Save & return to menu
                            </AsyncButton>
                        </div>
                    </section>
                </div>
            ) : null}
            {game.saving?.requested ? (
                <div className="scrim">
                    <section
                        className="pause-panel"
                        role="dialog"
                        aria-modal="true"
                        aria-label="Saving world"
                    >
                        <h1>
                            {game.saving.error
                                ? "Your world could not be saved"
                                : "Saving your world"}
                        </h1>
                        {game.saving.error ? (
                            <>
                                <p className="notice" role="alert">
                                    {game.saving.error}
                                </p>
                                <AsyncButton method="game.save">
                                    {game.saving.returnToShell
                                        ? "Retry save & return"
                                        : "Retry save"}
                                </AsyncButton>
                            </>
                        ) : (
                            <>
                                <p className="muted">
                                    {game.saving.returnToShell
                                        ? "Finishing writes before returning to the menu."
                                        : "Writing your latest progress to disk."}
                                </p>
                                <div className="loading-track" />
                            </>
                        )}
                    </section>
                </div>
            ) : null}
        </main>
    );
}
function AsyncButton({
    method,
    children,
}: {
    method: "game.save" | "game.exit";
    children: React.ReactNode;
}) {
    const [busy, setBusy] = useState(false);
    return (
        <button
            disabled={busy}
            onClick={async () => {
                setBusy(true);
                try {
                    await send(method, {});
                } catch {
                } finally {
                    setBusy(false);
                }
            }}
        >
            {busy ? "Saving…" : children}
        </button>
    );
}
const SlotButton = memo(function SlotButton({
    slot,
    active = false,
    selected = false,
    onClick,
    onDrop,
}: {
    slot: Slot;
    active?: boolean;
    selected?: boolean;
    onClick(): void;
    onDrop?(from: number): void;
}) {
    return (
        <button
            className={`slot ${active ? "active" : ""} ${selected ? "selected" : ""}`}
            aria-label={`Slot ${slot.index + 1}: ${slot.item ? `${slot.name}, ${slot.quantity}` : "empty"}`}
            aria-pressed={selected || active}
            title={
                slot.item
                    ? `${slot.name} × ${slot.quantity}`
                    : `Empty slot ${slot.index + 1}`
            }
            draggable={!!slot.item && !!onDrop}
            onDragStart={(e) =>
                e.dataTransfer.setData(
                    "application/x-lattice-slot",
                    String(slot.index),
                )
            }
            onDragOver={(e) => {
                if (onDrop) e.preventDefault();
            }}
            onDrop={(e) => {
                e.preventDefault();
                const raw = e.dataTransfer.getData(
                    "application/x-lattice-slot",
                );
                if (raw && Number.isSafeInteger(Number(raw)))
                    onDrop?.(Number(raw));
            }}
            onClick={onClick}
        >
            <span className="slot-key">
                {slot.index < 9 ? slot.index + 1 : ""}
            </span>
            {slot.item ? (
                <>
                    <ItemSwatch color={slot.color} name={slot.name} />
                    <span className="slot-quantity">{slot.quantity}</span>
                </>
            ) : null}
        </button>
    );
});
function ItemSwatch({ color, name }: { color: string; name: string }) {
    return (
        <span
            className="item-swatch"
            style={{ "--item-color": color } as React.CSSProperties}
            aria-hidden="true"
        >
            <span>{name.slice(0, 2)}</span>
        </span>
    );
}
function Inventory({ game }: { game: GameState }) {
    const [selected, setSelected] = useState<number | null>(null);
    const [query, setQuery] = useState(game.catalog?.query ?? "");
    const [category, setCategory] = useState(game.catalog?.category ?? "all");
    const [page, setPage] = useState(0);
    const deferred = useDeferredValue(query.trim().toLowerCase());
    const items = game.catalog
        ? game.items
        : game.items.filter(
              (i) =>
                  (category === "all" || i.category === category) &&
                  `${i.name} ${i.id}`.toLowerCase().includes(deferred),
          );
    const categories =
        game.catalog?.categories ??
        [...new Set(game.items.map((i) => i.category))].map((value) => ({
            id: value,
            name: value,
        }));
    const pageCount = Math.max(
        1,
        game.catalog?.pages ?? Math.ceil(items.length / 80),
    );
    const activePage = game.catalog?.page ?? Math.min(page, pageCount - 1);
    useEffect(() => {
        if (
            !game.catalog ||
            (query === game.catalog.query && category === game.catalog.category)
        )
            return;
        const timer = setTimeout(
            () => act("catalog.query", { query, category, page: 0 }),
            150,
        );
        return () => clearTimeout(timer);
    }, [query, category, game.catalog?.query, game.catalog?.category]);
    const changePage = (next: number) => {
        if (game.catalog) act("catalog.query", { page: next });
        else setPage(next);
    };
    const visibleItems = game.catalog
        ? items
        : items.slice(activePage * 80, (activePage + 1) * 80);
    const move = async (from: number, to: number) => {
        if (from === to) {
            setSelected(null);
            return;
        }
        try {
            await send("inventory.move", { from, to });
            setSelected(null);
        } catch {}
    };
    return (
        <div className="scrim">
            <section
                className="inventory-panel"
                role="dialog"
                aria-modal="true"
                aria-label={
                    game.overlay === "workbench" ? "Workbench" : "Inventory"
                }
            >
                <header className="panel-header">
                    <div>
                        <h1>
                            {game.overlay === "workbench"
                                ? "Workbench"
                                : "Inventory"}
                        </h1>
                        <p>
                            {selected === null
                                ? "Select a stack, then its destination. Or drag to move."
                                : `Moving slot ${selected + 1}. Choose a destination.`}
                        </p>
                    </div>
                    <button
                        className="close"
                        aria-label="Close inventory"
                        onClick={() => act("game.surface", { action: "back" })}
                    >
                        ×
                    </button>
                </header>
                {game.catalog?.error ? <p className="notice" role="alert">{game.catalog.error}</p> : null}
                <div className="inventory-body">
                    <section className="inventory-storage">
                        <h2>Backpack</h2>
                        <div className="slot-grid">
                            {game.slots.slice(9).map((slot) => (
                                <SlotButton
                                    key={slot.index}
                                    slot={slot}
                                    selected={slot.index === selected}
                                    onClick={() =>
                                        selected !== null
                                            ? void move(selected, slot.index)
                                            : slot.item
                                              ? setSelected(slot.index)
                                              : undefined
                                    }
                                    onDrop={(from) =>
                                        void move(from, slot.index)
                                    }
                                />
                            ))}
                        </div>
                        <h2 className="quick-title">Quick access</h2>
                        <div className="slot-grid">
                            {game.slots.slice(0, 9).map((slot) => (
                                <SlotButton
                                    key={slot.index}
                                    slot={slot}
                                    active={slot.index === game.hotbar}
                                    selected={slot.index === selected}
                                    onClick={() =>
                                        selected !== null
                                            ? void move(selected, slot.index)
                                            : slot.item
                                              ? setSelected(slot.index)
                                              : undefined
                                    }
                                    onDrop={(from) =>
                                        void move(from, slot.index)
                                    }
                                />
                            ))}
                        </div>
                        {selected !== null ? (
                            <button
                                className="text-button"
                                onClick={() => setSelected(null)}
                            >
                                Cancel move
                            </button>
                        ) : null}
                    </section>
                    <aside className="crafting">
                        <h2>Crafting</h2>
                        {game.recipes.length ? (
                            game.recipes.map((recipe) => (
                                <button
                                    key={recipe.id}
                                    className="recipe"
                                    disabled={!recipe.craftable}
                                    title={
                                        recipe.craftable
                                            ? "Craft recipe"
                                            : "Required materials are missing"
                                    }
                                    onClick={() =>
                                        act("recipe.craft", {
                                            recipe: recipe.id,
                                        })
                                    }
                                >
                                    <span>
                                        {recipe.name}
                                        <small>
                                            {recipe.craftable
                                                ? "Ready to craft"
                                                : "Missing materials"}
                                        </small>
                                    </span>
                                    <span aria-hidden="true">›</span>
                                </button>
                            ))
                        ) : (
                            <p className="muted">No recipes available here.</p>
                        )}
                        {game.catalog && game.catalog.recipePages > 1 ? (
                            <nav
                                className="catalog-pagination"
                                aria-label="Recipe pages"
                            >
                                <button
                                    disabled={game.catalog.recipePage === 0}
                                    onClick={() =>
                                        act("catalog.query", {
                                            recipePage:
                                                game.catalog!.recipePage - 1,
                                        })
                                    }
                                >
                                    Previous
                                </button>
                                <span>
                                    {game.catalog.recipePage + 1} /{" "}
                                    {game.catalog.recipePages}
                                </span>
                                <button
                                    disabled={
                                        game.catalog.recipePage + 1 >=
                                        game.catalog.recipePages
                                    }
                                    onClick={() =>
                                        act("catalog.query", {
                                            recipePage:
                                                game.catalog!.recipePage + 1,
                                        })
                                    }
                                >
                                    Next
                                </button>
                            </nav>
                        ) : null}
                    </aside>
                </div>
                <section className="creative-catalog">
                    <header>
                        <h2>Item catalogue</h2>
                        <input
                            aria-label="Search items"
                            placeholder="Search items…"
                            value={query}
                            onChange={(e) => {
                                setQuery(e.target.value);
                                setPage(0);
                            }}
                        />
                        <select
                            aria-label="Item category"
                            value={category}
                            onChange={(e) => {
                                setCategory(e.target.value);
                                setPage(0);
                            }}
                        >
                            <option value="all">All categories</option>
                            {categories
                                .filter((c) => c.id !== "all")
                                .map((c) => (
                                    <option key={c.id} value={c.id}>
                                        {c.name}
                                    </option>
                                ))}
                        </select>
                    </header>
                    <div className="catalog-items">
                        {visibleItems.map((item) => (
                            <CatalogItem
                                key={item.id}
                                item={item}
                                creative={game.creative}
                            />
                        ))}
                        {!items.length ? (
                            <p className="muted">No items match your search.</p>
                        ) : null}
                    </div>
                    {pageCount > 1 ? (
                        <nav
                            className="catalog-pagination"
                            aria-label="Catalogue pages"
                        >
                            <button
                                disabled={activePage === 0}
                                onClick={() => changePage(activePage - 1)}
                            >
                                Previous
                            </button>
                            <span>
                                Page {activePage + 1} of {pageCount} ·{" "}
                                {game.catalog?.total ?? items.length} items
                            </span>
                            <button
                                disabled={activePage + 1 === pageCount}
                                onClick={() => changePage(activePage + 1)}
                            >
                                Next
                            </button>
                        </nav>
                    ) : null}
                    {!game.creative ? (
                        <small>
                            Browse items here. Gather materials in the world to
                            use them.
                        </small>
                    ) : null}
                </section>
                <footer className="panel-footer">
                    <span className="muted">
                        {game.creative
                            ? "Creative inventory"
                            : "Survival inventory"}
                    </span>
                    <span>
                        <kbd>Esc</kbd> Close <kbd>Tab</kbd> Navigate
                    </span>
                </footer>
            </section>
        </div>
    );
}
function CatalogItem({ item, creative }: { item: Item; creative: boolean }) {
    return (
        <button
            className="catalog-item"
            aria-disabled={!creative}
            onClick={() => {
                if (creative) act("inventory.pick", { item: item.id });
            }}
            title={`${item.name}\n${item.id}\n${creative ? "Add to inventory" : "View only in survival"}`}
        >
            <ItemSwatch color={item.color} name={item.name} />
            <span>{item.name}</span>
        </button>
    );
}
