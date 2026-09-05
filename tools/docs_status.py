#!/usr/bin/env python3
"""Validate Lattice Axiom documentation traceability and render its status page."""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"
STATUS_PATH = DOCS / "delivery" / "status.md"
TEMPLATES = DOCS / "meta" / "templates"

DOCUMENT_STATUSES = {"exploration", "proposed", "accepted", "active", "superseded"}
DOCUMENT_TYPES = {
    "index",
    "overview",
    "package-spec",
    "platform-spec",
    "decision",
    "research",
    "roadmap",
    "plan",
    "reference",
    "meta",
}
EVIDENCE_STATUSES = {"scaffolded", "partial", "verified"}
REQUIREMENT_PATTERN = re.compile(r"^[A-Z][A-Z0-9]+(?:-[A-Z0-9]+)+$")
DOCUMENT_ID_PATTERN = re.compile(r"^[a-z0-9]+(?:[.-][a-z0-9]+)*$")
COMMIT_PATTERN = re.compile(r"^[0-9a-f]{40}$")
MARKDOWN_LINK_PATTERN = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")


@dataclass(frozen=True)
class Document:
    path: Path
    title: str
    document_id: str
    document_status: str
    document_type: str
    tracks_implementation: bool
    owners: tuple[str, ...]
    requirements: tuple[str, ...]
    updated: str
    decisions: tuple[str, ...]


@dataclass(frozen=True)
class Requirement:
    requirement_id: str
    document_id: str
    owner: str
    level: str
    statement: str
    acceptance: tuple[str, ...]
    source: Path


@dataclass(frozen=True)
class Evidence:
    requirement_id: str
    status: str
    repository: str
    commit: str
    production_paths: tuple[str, ...]
    product_entries: tuple[str, ...]
    checks: tuple[str, ...]
    notes: str
    source: Path


class ValidationErrors:
    def __init__(self) -> None:
        self.messages: list[str] = []

    def add(self, path: Path | str, message: str) -> None:
        display = path if isinstance(path, str) else path.relative_to(ROOT).as_posix()
        self.messages.append(f"{display}: {message}")

    def raise_if_any(self) -> None:
        if not self.messages:
            return
        for message in sorted(self.messages):
            print(f"error: {message}", file=sys.stderr)
        raise SystemExit(1)


def scalar(value: str) -> str | bool:
    value = value.strip()
    if value == "true":
        return True
    if value == "false":
        return False
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {'"', "'"}:
        return value[1:-1]
    return value


def parse_frontmatter(path: Path, errors: ValidationErrors) -> dict[str, Any] | None:
    text = path.read_text(encoding="utf-8").replace("\r\n", "\n")
    if not text.startswith("---\n"):
        errors.add(path, "missing YAML frontmatter")
        return None
    end = text.find("\n---\n", 4)
    if end < 0:
        errors.add(path, "unterminated YAML frontmatter")
        return None

    fields: dict[str, Any] = {}
    active_list: str | None = None
    for line_number, line in enumerate(text[4:end].splitlines(), start=2):
        if not line.strip():
            continue
        if line.startswith("  - "):
            if active_list is None:
                errors.add(path, f"orphan list item at frontmatter line {line_number}")
                continue
            fields[active_list].append(scalar(line[4:]))
            continue
        if line.startswith((" ", "\t")) or ":" not in line:
            errors.add(path, f"unsupported frontmatter syntax at line {line_number}")
            active_list = None
            continue
        key, raw_value = line.split(":", 1)
        if key in fields:
            errors.add(path, f"duplicate frontmatter field {key}")
        if raw_value.strip():
            fields[key] = scalar(raw_value)
            active_list = None
        else:
            fields[key] = []
            active_list = key
    return fields


def markdown_files() -> list[Path]:
    return [
        path
        for path in sorted(DOCS.rglob("*.md"))
        if TEMPLATES not in path.parents
    ]


def require_string(fields: dict[str, Any], key: str, path: Path, errors: ValidationErrors) -> str:
    value = fields.get(key)
    if not isinstance(value, str) or not value:
        errors.add(path, f"{key} must be a non-empty string")
        return ""
    return value


def require_string_list(
    fields: dict[str, Any], key: str, path: Path, errors: ValidationErrors
) -> tuple[str, ...]:
    value = fields.get(key, [])
    if not isinstance(value, list) or not all(isinstance(item, str) and item for item in value):
        errors.add(path, f"{key} must be a list of non-empty strings")
        return ()
    if len(value) != len(set(value)):
        errors.add(path, f"{key} contains duplicates")
    return tuple(value)


def load_documents(errors: ValidationErrors) -> dict[str, Document]:
    documents: dict[str, Document] = {}
    for path in markdown_files():
        fields = parse_frontmatter(path, errors)
        if fields is None:
            continue
        for legacy in ("status", "type"):
            if legacy in fields:
                errors.add(path, f"legacy frontmatter field {legacy} is forbidden")

        title = require_string(fields, "title", path, errors)
        document_id = require_string(fields, "document_id", path, errors)
        document_status = require_string(fields, "document_status", path, errors)
        document_type = require_string(fields, "document_type", path, errors)
        updated = require_string(fields, "updated", path, errors)
        tracks = fields.get("tracks_implementation")
        if not isinstance(tracks, bool):
            errors.add(path, "tracks_implementation must be true or false")
            tracks = False
        owners = require_string_list(fields, "owners", path, errors)
        requirements = require_string_list(fields, "requirements", path, errors)
        decisions = require_string_list(fields, "decision", path, errors)

        if document_id and not DOCUMENT_ID_PATTERN.fullmatch(document_id):
            errors.add(path, f"invalid document_id {document_id}")
        if document_id in documents:
            errors.add(path, f"duplicate document_id also used by {documents[document_id].path}")
        if document_status not in DOCUMENT_STATUSES:
            errors.add(path, f"invalid document_status {document_status}")
        if document_type not in DOCUMENT_TYPES:
            errors.add(path, f"invalid document_type {document_type}")
        if tracks and not requirements:
            errors.add(path, "tracked document has no requirement IDs")
        if not tracks and requirements:
            errors.add(path, "untracked document must not list requirements")
        if not re.fullmatch(r"\d{4}-\d{2}-\d{2}|YYYY-MM-DD", updated):
            errors.add(path, f"invalid updated date {updated}")

        for decision in decisions:
            target = (path.parent / decision).resolve()
            if not target.is_file() or DOCS / "decisions" not in target.parents:
                errors.add(path, f"broken decision reference {decision}")

        if document_id:
            documents[document_id] = Document(
                path=path,
                title=title,
                document_id=document_id,
                document_status=document_status,
                document_type=document_type,
                tracks_implementation=tracks,
                owners=owners,
                requirements=requirements,
                updated=updated,
                decisions=decisions,
            )
    return documents


def validate_markdown_links(errors: ValidationErrors) -> None:
    for path in markdown_files():
        text = path.read_text(encoding="utf-8")
        for match in MARKDOWN_LINK_PATTERN.finditer(text):
            raw_target = match.group(1).strip()
            if raw_target.startswith("<") and raw_target.endswith(">"):
                raw_target = raw_target[1:-1]
            if (
                not raw_target
                or raw_target.startswith("#")
                or re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", raw_target)
            ):
                continue
            file_target = raw_target.partition("#")[0]
            target = path.parent / file_target
            if not target.exists():
                errors.add(path, f"broken Markdown link {raw_target}")


def load_json(path: Path, errors: ValidationErrors) -> dict[str, Any] | None:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        errors.add(path, f"invalid JSON: {error}")
        return None
    if not isinstance(value, dict):
        errors.add(path, "JSON root must be an object")
        return None
    return value


def validate_schema_reference(path: Path, payload: dict[str, Any], errors: ValidationErrors) -> None:
    schema = payload.get("$schema")
    if not isinstance(schema, str) or not schema:
        errors.add(path, "missing $schema")
        return
    if re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", schema):
        return
    if not (path.parent / schema).is_file():
        errors.add(path, f"broken $schema reference {schema}")


def load_requirements(
    documents: dict[str, Document], errors: ValidationErrors
) -> dict[str, Requirement]:
    requirements: dict[str, Requirement] = {}
    for path in sorted(DOCS.rglob("requirements.json")):
        payload = load_json(path, errors)
        if payload is None:
            continue
        validate_schema_reference(path, payload, errors)
        if payload.get("schema_version") != 1:
            errors.add(path, "schema_version must be 1")
        owner = payload.get("owner")
        rows = payload.get("requirements")
        if not isinstance(owner, str) or not owner:
            errors.add(path, "owner must be a non-empty string")
            continue
        if not isinstance(rows, list):
            errors.add(path, "requirements must be an array")
            continue
        for index, row in enumerate(rows):
            label = f"requirements[{index}]"
            if not isinstance(row, dict):
                errors.add(path, f"{label} must be an object")
                continue
            requirement_id = row.get("id")
            document_id = row.get("document_id")
            level = row.get("level")
            statement = row.get("statement")
            acceptance = row.get("acceptance")
            if not isinstance(requirement_id, str) or not REQUIREMENT_PATTERN.fullmatch(requirement_id):
                errors.add(path, f"{label}.id is invalid")
                continue
            if requirement_id in requirements:
                errors.add(path, f"duplicate requirement ID {requirement_id}")
                continue
            if not isinstance(document_id, str) or document_id not in documents:
                errors.add(path, f"{requirement_id} references unknown document {document_id}")
                continue
            if level not in {"must", "should"}:
                errors.add(path, f"{requirement_id} has invalid level {level}")
                continue
            if not isinstance(statement, str) or not statement:
                errors.add(path, f"{requirement_id} has an empty statement")
                continue
            if not isinstance(acceptance, list) or not acceptance or not all(
                isinstance(item, str) and item for item in acceptance
            ):
                errors.add(path, f"{requirement_id} must have non-empty acceptance strings")
                continue
            requirements[requirement_id] = Requirement(
                requirement_id=requirement_id,
                document_id=document_id,
                owner=owner,
                level=level,
                statement=statement,
                acceptance=tuple(acceptance),
                source=path,
            )

    for document in documents.values():
        for requirement_id in document.requirements:
            if requirement_id not in requirements:
                errors.add(document.path, f"unknown requirement ID {requirement_id}")
    return requirements


def relative_implementation_path(value: Any) -> bool:
    return (
        isinstance(value, str)
        and bool(value)
        and not Path(value).is_absolute()
        and "\\" not in value
        and ".." not in Path(value).parts
    )


def load_evidence(
    requirements: dict[str, Requirement], errors: ValidationErrors
) -> dict[str, Evidence]:
    evidence: dict[str, Evidence] = {}
    evidence_directory = DOCS / "delivery" / "evidence"
    for path in sorted(evidence_directory.glob("*.json")) if evidence_directory.exists() else []:
        payload = load_json(path, errors)
        if payload is None:
            continue
        validate_schema_reference(path, payload, errors)
        if payload.get("schema_version") != 1:
            errors.add(path, "schema_version must be 1")
        implementation = payload.get("implementation")
        rows = payload.get("evidence")
        if not isinstance(implementation, dict):
            errors.add(path, "implementation must be an object")
            continue
        repository = implementation.get("repository")
        commit = implementation.get("commit")
        if not isinstance(repository, str) or not repository.startswith("https://"):
            errors.add(path, "implementation.repository must be an https URL")
            continue
        if not isinstance(commit, str) or not COMMIT_PATTERN.fullmatch(commit):
            errors.add(path, "implementation.commit must be a full lowercase Git commit")
            continue
        if not isinstance(rows, list):
            errors.add(path, "evidence must be an array")
            continue
        for index, row in enumerate(rows):
            label = f"evidence[{index}]"
            if not isinstance(row, dict):
                errors.add(path, f"{label} must be an object")
                continue
            requirement_id = row.get("requirement_id")
            status = row.get("status")
            production_paths = row.get("production_paths")
            product_entries = row.get("product_entries")
            checks = row.get("checks")
            notes = row.get("notes", "")
            if requirement_id not in requirements:
                errors.add(path, f"{label} references unknown requirement {requirement_id}")
                continue
            if requirement_id in evidence:
                errors.add(path, f"duplicate active evidence for {requirement_id}")
                continue
            if status not in EVIDENCE_STATUSES:
                errors.add(path, f"{requirement_id} has invalid evidence status {status}")
                continue
            arrays = {
                "production_paths": production_paths,
                "product_entries": product_entries,
                "checks": checks,
            }
            invalid_array = False
            for name, value in arrays.items():
                if not isinstance(value, list) or not all(isinstance(item, str) and item for item in value):
                    errors.add(path, f"{requirement_id}.{name} must contain strings")
                    invalid_array = True
                elif len(value) != len(set(value)):
                    errors.add(path, f"{requirement_id}.{name} contains duplicates")
            if invalid_array:
                continue
            if not all(relative_implementation_path(item) for item in production_paths):
                errors.add(path, f"{requirement_id} has an unsafe production path")
            if status == "verified" and not (production_paths and product_entries and checks):
                errors.add(path, f"verified {requirement_id} lacks production path, entry or checks")
            if not isinstance(notes, str):
                errors.add(path, f"{requirement_id}.notes must be a string")
                notes = ""
            evidence[requirement_id] = Evidence(
                requirement_id=requirement_id,
                status=status,
                repository=repository,
                commit=commit,
                production_paths=tuple(production_paths),
                product_entries=tuple(product_entries),
                checks=tuple(checks),
                notes=notes,
                source=path,
            )
    return evidence


def aggregate_status(
    document: Document,
    requirements: dict[str, Requirement],
    evidence: dict[str, Evidence],
) -> tuple[str, int, int, tuple[str, ...], tuple[str, ...]]:
    if not document.tracks_implementation:
        return "not-applicable", 0, 0, (), ()
    must_ids = tuple(
        requirement_id
        for requirement_id in document.requirements
        if requirement_id in requirements and requirements[requirement_id].level == "must"
    )
    statuses = [evidence[item].status for item in must_ids if item in evidence]
    verified = sum(status == "verified" for status in statuses)
    if must_ids and verified == len(must_ids):
        state = "implemented"
    elif any(status in {"partial", "verified"} for status in statuses):
        state = "in-progress"
    elif any(status == "scaffolded" for status in statuses):
        state = "scaffolded"
    else:
        state = "not-started"
    owners = tuple(sorted({requirements[item].owner for item in must_ids if item in requirements}))
    commits = tuple(sorted({evidence[item].commit for item in must_ids if item in evidence}))
    return state, verified, len(must_ids), owners, commits


def markdown_escape(value: str) -> str:
    return value.replace("|", "\\|").replace("\n", " ")


def document_link(document: Document) -> str:
    relative = os.path.relpath(document.path, STATUS_PATH.parent).replace("\\", "/")
    return f"[{markdown_escape(document.title)}]({relative})"


def render_status(
    documents: dict[str, Document],
    requirements: dict[str, Requirement],
    evidence: dict[str, Evidence],
) -> str:
    updated = max(
        (document.updated for document in documents.values() if document.updated != "YYYY-MM-DD"),
        default="2026-08-22",
    )
    baselines = sorted({(item.repository, item.commit) for item in evidence.values()})
    lines = [
        "---",
        "title: Implementation status",
        "document_id: delivery.status",
        "document_status: active",
        "document_type: index",
        "tracks_implementation: false",
        f"updated: {updated}",
        "---",
        "",
        "# Implementation status",
        "",
        "> Generated by `python tools/docs_status.py generate`; do not edit status tables by hand.",
        "",
        "`implemented` requires every MUST requirement to have `verified` evidence with a production",
        "path, product entry and reproducible checks. All other states remain conservative.",
        "",
        "## Evidence baselines",
        "",
        "| Implementation repository | Commit |",
        "| --- | --- |",
    ]
    if baselines:
        for repository, commit in baselines:
            lines.append(f"| [{repository}]({repository}) | `{commit}` |")
    else:
        lines.append("| — | — |")

    lines.extend(
        [
            "",
            "## Implementation-tracked documents",
            "",
            "| Document | Owner | State | Verified MUST | Evidence commit |",
            "| --- | --- | --- | ---: | --- |",
        ]
    )
    tracked = sorted(
        (document for document in documents.values() if document.tracks_implementation),
        key=lambda item: item.path.as_posix(),
    )
    for document in tracked:
        state, verified, total, owners, commits = aggregate_status(document, requirements, evidence)
        owner = ", ".join(f"`{markdown_escape(item)}`" for item in owners) or "—"
        commit = ", ".join(f"`{item[:12]}`" for item in commits) or "—"
        lines.append(
            f"| {document_link(document)} | {owner} | `{state}` | {verified}/{total} | {commit} |"
        )

    lines.extend(
        [
            "",
            "## Documents without implementation claims",
            "",
            "| Document | Type | State |",
            "| --- | --- | --- |",
        ]
    )
    untracked = sorted(
        (document for document in documents.values() if not document.tracks_implementation),
        key=lambda item: item.path.as_posix(),
    )
    for document in untracked:
        lines.append(
            f"| {document_link(document)} | `{document.document_type}` | `not-applicable` |"
        )
    lines.append("")
    return "\n".join(lines)


def validate_all() -> tuple[dict[str, Document], dict[str, Requirement], dict[str, Evidence]]:
    errors = ValidationErrors()
    documents = load_documents(errors)
    validate_markdown_links(errors)
    requirements = load_requirements(documents, errors)
    evidence = load_evidence(requirements, errors)
    errors.raise_if_any()
    return documents, requirements, evidence


def command_generate() -> None:
    documents, requirements, evidence = validate_all()
    STATUS_PATH.write_text(
        render_status(documents, requirements, evidence), encoding="utf-8", newline="\n"
    )
    print(f"generated {STATUS_PATH.relative_to(ROOT).as_posix()}")


def command_check() -> None:
    documents, requirements, evidence = validate_all()
    expected = render_status(documents, requirements, evidence)
    actual = STATUS_PATH.read_text(encoding="utf-8").replace("\r\n", "\n")
    if actual != expected:
        print(
            "error: docs/delivery/status.md is stale; run "
            "`python tools/docs_status.py generate`",
            file=sys.stderr,
        )
        raise SystemExit(1)
    print(
        f"validated {len(documents)} documents, {len(requirements)} requirements and "
        f"{len(evidence)} evidence rows"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("generate", "check"))
    arguments = parser.parse_args()
    if arguments.command == "generate":
        command_generate()
    else:
        command_check()


if __name__ == "__main__":
    main()
