import { useDeferredValue, useState } from "react";
import { act, send } from "./bridge";
import type { BindingRow, Json, SettingRow, SettingsState } from "./types";

export function Settings({ settings }: { settings: SettingsState }) {
    const [owner, setOwner] = useState("all");
    const [category, setCategory] = useState("all");
    const [query, setQuery] = useState("");
    const [pending, setPending] = useState(false);
    const search = useDeferredValue(query.trim().toLowerCase());
    const owners =
        settings.owners ??
        [...new Set(settings.rows.map((row) => row.owner))].map((id) => ({
            id,
            name: id,
        }));
    const categories = [
        ...new Set(
            settings.rows
                .filter((row) => owner === "all" || row.owner === owner)
                .map((row) => row.category),
        ),
    ];
    const rows = settings.rows.filter(
        (row) =>
            row.visible !== false &&
            (owner === "all" || row.owner === owner) &&
            (category === "all" || row.category === category) &&
            `${row.label} ${row.description} ${row.owner} ${row.id}`
                .toLowerCase()
                .includes(search),
    );
    const bindings =
        owner === "all" && (category === "all" || category === "controls")
            ? (settings.bindings ?? []).filter((row) =>
                  `${row.label} ${row.id} ${row.context}`
                      .toLowerCase()
                      .includes(search),
              )
            : [];
    const apply = async () => {
        setPending(true);
        try {
            await send("settings.apply", {});
        } catch {
        } finally {
            setPending(false);
        }
    };
    return (
        <div className="scrim settings-scrim">
            <section
                className="settings-panel"
                role="dialog"
                aria-modal="true"
                aria-labelledby="settings-title"
            >
                <header className="panel-header">
                    <div>
                        <h1 id="settings-title">Settings & mods</h1>
                        <p>Make this world feel like yours.</p>
                    </div>
                    <button
                        className="close"
                        aria-label="Close settings"
                        onClick={() => act("settings.cancel", {})}
                    >
                        ×
                    </button>
                </header>
                <div className="settings-body">
                    <aside className="settings-nav">
                        <input
                            aria-label="Search settings"
                            placeholder="Search settings…"
                            value={query}
                            onChange={(e) => setQuery(e.target.value)}
                        />
                        <h2>Packages</h2>
                        <nav aria-label="Setting owners">
                            <button
                                className={owner === "all" ? "selected" : ""}
                                onClick={() => {
                                    setOwner("all");
                                    setCategory("all");
                                }}
                            >
                                All packages
                            </button>
                            {owners.map((entry) => (
                                <button
                                    title={entry.id}
                                    key={entry.id}
                                    className={
                                        owner === entry.id ? "selected" : ""
                                    }
                                    onClick={() => {
                                        setOwner(entry.id);
                                        setCategory("all");
                                    }}
                                >
                                    {entry.name}
                                </button>
                            ))}
                        </nav>
                        <h2>Categories</h2>
                        <nav aria-label="Setting categories">
                            <button
                                className={category === "all" ? "selected" : ""}
                                onClick={() => setCategory("all")}
                            >
                                All settings
                            </button>
                            {categories.map((value) => (
                                <button
                                    key={value}
                                    className={
                                        category === value ? "selected" : ""
                                    }
                                    onClick={() => setCategory(value)}
                                >
                                    {humanize(value)}
                                </button>
                            ))}
                        </nav>
                    </aside>
                    <div className="settings-fields">
                        <h2>
                            {owner === "all"
                                ? "All settings"
                                : (owners.find((entry) => entry.id === owner)
                                      ?.name ?? owner)}
                        </h2>
                        {settings.message ? (
                            <p className="notice" role="status">
                                {settings.message}
                            </p>
                        ) : null}
                        {rows.map((row) => (
                            <Setting key={row.id} row={row} />
                        ))}
                        {bindings.length ? (
                            <section className="bindings">
                                <h2>Keyboard & mouse</h2>
                                {bindings.map((row) => (
                                    <Binding key={row.id} row={row} />
                                ))}
                            </section>
                        ) : null}
                        {!rows.length && !bindings.length ? (
                            <div className="empty">
                                <h3>
                                    {search
                                        ? "No matching settings"
                                        : "No settings exposed"}
                                </h3>
                                <p>
                                    {search
                                        ? "Try a shorter search or choose another category."
                                        : "This package does not expose configurable settings for this client."}
                                </p>
                            </div>
                        ) : null}
                    </div>
                </div>
                <footer className="panel-footer">
                    <button
                        disabled={pending || !rows.length}
                        onClick={() =>
                            act(
                                "settings.reset",
                                owner === "all" ? {} : { owner },
                            )
                        }
                    >
                        Reset defaults
                    </button>
                    <span className="draft-status" role="status">
                        {settings.dirty
                            ? "Unsaved changes"
                            : "All changes saved"}
                    </span>
                    <div className="footer-actions">
                        <button
                            disabled={pending}
                            onClick={() => act("settings.cancel", {})}
                        >
                            Cancel
                        </button>
                        <button
                            className="primary"
                            disabled={!settings.dirty || pending}
                            onClick={() => void apply()}
                        >
                            {pending ? "Applying…" : "Apply changes"}
                        </button>
                    </div>
                </footer>
            </section>
        </div>
    );
}
function Binding({ row }: { row: BindingRow }) {
    const [capturing, setCapturing] = useState(false);
    const label =
        row.display ??
        (row.code ? [...row.modifiers, row.code].join(" + ") : "Unbound");
    return (
        <div className="setting-row">
            <div className="setting-copy">
                <label htmlFor={`binding-${row.id}`}>{row.label}</label>
                <p>{row.description || humanize(row.context)}</p>
                {!row.editable ? <small>Defined by this package</small> : null}
            </div>
            <div className="setting-input">
                <button
                    id={`binding-${row.id}`}
                    data-key-capture={capturing}
                    className="key-binding"
                    disabled={!row.editable}
                    onClick={() => setCapturing(true)}
                    onBlur={() => setCapturing(false)}
                    onKeyDown={(event) => {
                        if (!capturing) return;
                        event.stopPropagation();
                        event.preventDefault();
                        if (
                            event.key !== "Escape" &&
                            !["Shift", "Control", "Alt", "Meta"].includes(
                                event.key,
                            )
                        ) {
                            const modifiers = [
                                event.shiftKey ? "shift" : "",
                                event.ctrlKey ? "control" : "",
                                event.altKey ? "alt" : "",
                                event.metaKey ? "super" : "",
                            ].filter(Boolean);
                            act("settings.bind", {
                                id: row.id,
                                code: event.code,
                                modifiers,
                            });
                            setCapturing(false);
                        } else if (event.key === "Escape") setCapturing(false);
                    }}
                >
                    {capturing ? "Press a key…" : label}
                </button>
                <button
                    className="reset-setting"
                    disabled={!row.editable}
                    title="Restore package binding"
                    aria-label={`Reset ${row.label} binding`}
                    onClick={() => act("settings.resetBinding", { id: row.id })}
                >
                    ↺
                </button>
                <button
                    className="reset-setting"
                    disabled={!row.editable}
                    title="Remove binding"
                    aria-label={`Unbind ${row.label}`}
                    onClick={() => act("settings.unbind", { id: row.id })}
                >
                    ×
                </button>
            </div>
        </div>
    );
}
function humanize(text: string) {
    return text.replace(/[-_.]/g, " ").replace(/^\w/, (c) => c.toUpperCase());
}
function Setting({ row }: { row: SettingRow }) {
    return (
        <div className={`setting-row ${!row.editable ? "read-only" : ""}`}>
            <div className="setting-copy">
                <label htmlFor={`setting-${row.id}`}>{row.label}</label>
                <p>{row.description}</p>
                <small>
                    {row.owner}
                    {row.impact && row.impact !== "immediate"
                        ? ` · ${humanize(row.impact)}`
                        : ""}
                </small>
                {row.reason ? (
                    <p className="setting-reason">{row.reason}</p>
                ) : null}
            </div>
            <div className="setting-input">
                <SettingControl row={row} />
                <button
                    className="reset-setting"
                    aria-label={`Reset ${row.label}`}
                    title="Reset this setting"
                    disabled={
                        !row.editable ||
                        JSON.stringify(row.value) ===
                            JSON.stringify(row.defaultValue)
                    }
                    onClick={() => act("settings.reset", { id: row.id })}
                >
                    ↺
                </button>
            </div>
        </div>
    );
}
function SettingControl({ row }: { row: SettingRow }) {
    const id = `setting-${row.id}`;
    const set = (value: Json) => act("settings.set", { id: row.id, value });
    const schema = row.schema;
    if (schema.type === "bool")
        return (
            <button
                id={id}
                className="toggle"
                role="switch"
                aria-checked={row.value === true}
                aria-label={row.label}
                disabled={!row.editable}
                onClick={() => set(!row.value)}
            >
                <span />
            </button>
        );
    if (schema.type === "enum")
        return (
            <select
                id={id}
                value={String(row.value)}
                disabled={!row.editable}
                onChange={(e) => set(e.target.value)}
            >
                {schema.values?.map((value) => (
                    <option key={value} value={value}>
                        {humanize(value)}
                    </option>
                ))}
            </select>
        );
    if (schema.type === "integer" || schema.type === "number")
        return <NumberControl row={row} set={set} />;
    if (schema.type === "key-binding")
        return <KeyBinding row={row} set={set} />;
    if (schema.type === "color" && Array.isArray(row.value))
        return (
            <div className="color-channels">
                {row.value.map((channel, index) => (
                    <label key={index}>
                        {["R", "G", "B", "A"][index]}
                        <input
                            type="number"
                            aria-label={`${row.label} ${["red", "green", "blue", "alpha"][index]}`}
                            min={0}
                            max={1}
                            step={0.01}
                            disabled={!row.editable}
                            value={Number(channel)}
                            onChange={(e) => {
                                const values = [...(row.value as Json[])];
                                values[index] = Number(e.target.value);
                                set(values);
                            }}
                        />
                    </label>
                ))}
            </div>
        );
    if (schema.type === "string") return <TextControl row={row} set={set} />;
    return (
        <output id={id}>
            {typeof row.value === "string"
                ? row.value
                : JSON.stringify(row.value)}
        </output>
    );
}
function NumberControl({
    row,
    set,
}: {
    row: SettingRow;
    set(value: Json): void;
}) {
    const [draft, setDraft] = useState<string | null>(null);
    const schema = row.schema;
    const commit = (value: string) => {
        const numeric = Number(value);
        if (value.trim() && Number.isFinite(numeric)) set(numeric);
        setDraft(null);
    };
    return (
        <div className="number-control">
            {schema.min != null && schema.max != null ? (
                <input
                    type="range"
                    aria-label={`${row.label} slider`}
                    min={schema.min}
                    max={schema.max}
                    step={
                        schema.step ?? (schema.type === "integer" ? 1 : "any")
                    }
                    value={draft ?? Number(row.value)}
                    disabled={!row.editable}
                    onChange={(e) => setDraft(e.target.value)}
                    onPointerUp={(e) => commit(e.currentTarget.value)}
                    onKeyUp={(e) => commit(e.currentTarget.value)}
                />
            ) : null}
            <input
                id={`setting-${row.id}`}
                aria-label={row.label}
                type="number"
                min={schema.min ?? undefined}
                max={schema.max ?? undefined}
                step={schema.step ?? (schema.type === "integer" ? 1 : "any")}
                value={draft ?? String(row.value)}
                disabled={!row.editable}
                onChange={(e) => setDraft(e.target.value)}
                onBlur={(e) => {
                    if (draft !== null) commit(e.target.value);
                }}
                onKeyDown={(e) => {
                    if (e.key === "Enter") commit(e.currentTarget.value);
                }}
            />
        </div>
    );
}
function TextControl({
    row,
    set,
}: {
    row: SettingRow;
    set(value: Json): void;
}) {
    const [draft, setDraft] = useState<string | null>(null);
    return (
        <input
            id={`setting-${row.id}`}
            value={draft ?? String(row.value)}
            disabled={!row.editable}
            minLength={row.schema.min_length ?? undefined}
            maxLength={row.schema.max_length ?? undefined}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={(e) => {
                if (draft !== null) {
                    set(e.target.value);
                    setDraft(null);
                }
            }}
            onKeyDown={(e) => {
                if (e.key === "Enter") {
                    set(e.currentTarget.value);
                    setDraft(null);
                }
            }}
        />
    );
}
function KeyBinding({ row, set }: { row: SettingRow; set(value: Json): void }) {
    const [capturing, setCapturing] = useState(false);
    return (
        <button
            id={`setting-${row.id}`}
            data-key-capture={capturing}
            disabled={!row.editable}
            className="key-binding"
            onBlur={() => setCapturing(false)}
            onClick={() => setCapturing(true)}
            onKeyDown={(e) => {
                if (!capturing) return;
                e.preventDefault();
                e.stopPropagation();
                if (e.key !== "Escape") set(e.code);
                setCapturing(false);
            }}
        >
            {capturing ? "Press a key… (Esc cancels)" : String(row.value)}
        </button>
    );
}
