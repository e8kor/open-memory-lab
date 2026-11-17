#!/usr/bin/env python3
"""Write docs/data/postgres-schema.md from the migrations, applied to a throwaway PostgreSQL database.

The page is read from the catalog after the last file under migrations/ has run, never from the SQL
text, so a column a later migration renamed, a check it dropped or a grant it revoked shows as the
database holds it. The script mints three login roles and one database for the run, applies every
migration as the owner role with `oml.serving_role` set to the serving role (as the lead does at
start), runs the lake writer's own GRANT statements from crates/bins/oml-lakewriter/src/source.rs
under its role, reads the catalog, and drops the database and the roles again. The minted names
never reach the page: each role is shown as owner, serving or lake writer.

It needs a PostgreSQL superuser URL and the `psql` client. CI runs it in the job that has a
PostgreSQL service; the scripts' own tests render from a catalog snapshot and need neither.

    python scripts/gen_postgres_schema.py [--database-url URL] [--check]

URL defaults to OML_SCHEMA_DATABASE_URL, then DATABASE_URL.
"""

from __future__ import annotations

import argparse
import difflib
import json
import os
import pathlib
import re
import subprocess
import sys
import time
import urllib.parse

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import docgen  # noqa: E402

MIGRATIONS = docgen.ROOT / "migrations"
LAKE_SOURCE = docgen.ROOT / "crates" / "bins" / "oml-lakewriter" / "src" / "source.rs"
OUT = docgen.DOCS / "data" / "postgres-schema.md"
SCRIPT = "scripts/gen_postgres_schema.py"
PASSWORD = "oml_schema_page_only"
ROLES = ("owner", "serving", "lake writer")
SELF_LINK = re.compile(r"\s*Owning page: docs/data/postgres-schema\.md\.?")
# The bookkeeping table sqlx creates before the first migration (sqlx-postgres 0.8, migrate.rs);
# 0007 revokes it from PUBLIC, so it must exist as the server's run leaves it.
SQLX_TABLE = """CREATE TABLE IF NOT EXISTS _sqlx_migrations (
    version BIGINT PRIMARY KEY,
    description TEXT NOT NULL,
    installed_on TIMESTAMPTZ NOT NULL DEFAULT now(),
    success BOOLEAN NOT NULL,
    checksum BYTEA NOT NULL,
    execution_time BIGINT NOT NULL
)"""
# Purposes for the tables no migration creates, so none carries a COMMENT.
NOTES = {
    "_sqlx_migrations": "The record of applied migrations, created by sqlx before the first migration runs. "
    "It is the one table without row-level security: it holds no tenant's rows, and only the owner reaches it.",
}
PRIVILEGE_ORDER = ["SELECT", "INSERT", "UPDATE", "DELETE", "TRUNCATE", "REFERENCES", "TRIGGER", "MAINTAIN", "USAGE"]


# ---------------------------------------------------------------------------
# The run: a throwaway database, the migrations, the catalog
# ---------------------------------------------------------------------------


def migration_files() -> list[pathlib.Path]:
    return sorted(MIGRATIONS.glob("[0-9][0-9][0-9][0-9]_*.sql"))


def lake_grants() -> list[str]:
    """The GRANT statements oml-lakewriter runs at its start, with `{role}` left in each."""
    src = LAKE_SOURCE.read_text()
    body = re.search(r"for stmt in \[(.*?)\]\s*\{", src, re.S)
    if not body:
        sys.exit(f"{LAKE_SOURCE.relative_to(docgen.ROOT)}: the lake writer's GRANT list was not found")
    stmts = []
    for call in re.findall(r"format!\((.*?)\),\s*$", body.group(1), re.S | re.M):
        literals = re.findall(r'"((?:[^"\\]|\\[\s\S])*)"', call)
        text = re.sub(r"\\\n\s*", "", literals[0])
        for arg in literals[1:]:
            text = text.replace("{}", arg, 1)
        if not text.startswith("GRANT ") or "{role}" not in text:
            sys.exit(f"{LAKE_SOURCE.relative_to(docgen.ROOT)}: unexpected lake writer statement {text!r}")
        stmts.append(text)
    if not stmts:
        sys.exit(f"{LAKE_SOURCE.relative_to(docgen.ROOT)}: the lake writer's GRANT list is empty")
    return stmts


def with_user(url: str, user: str | None, database: str | None) -> tuple[str, dict]:
    """`url` with its user and database replaced, and the environment psql needs for it."""
    parts = urllib.parse.urlsplit(url)
    host = parts.hostname or ""
    netloc = urllib.parse.quote(user or parts.username or "") + "@" + (f"[{host}]" if ":" in host else host)
    if parts.port:
        netloc += f":{parts.port}"
    path = "/" + (database if database is not None else parts.path.lstrip("/"))
    env = {k: v for k, v in os.environ.items() if not k.startswith("PG")}
    password = PASSWORD if user else urllib.parse.unquote(parts.password or "")
    if password:
        env["PGPASSWORD"] = password
    return urllib.parse.urlunsplit((parts.scheme, netloc, path, parts.query, "")), env


def psql(url: str, env: dict, *args: str) -> str:
    run = subprocess.run(
        ["psql", "--no-psqlrc", "-X", "-q", "-v", "ON_ERROR_STOP=1", "-At", "-d", url, *args],
        env=env, capture_output=True, text=True,
    )
    if run.returncode != 0:
        raise RuntimeError(run.stderr.strip() or f"psql exited {run.returncode}")
    return run.stdout


CATALOG_SQL = r"""
WITH rel AS (
    SELECT c.oid, c.relname, c.relkind, c.relrowsecurity, c.relforcerowsecurity, c.relacl,
           pg_get_userbyid(c.relowner) AS owner
      FROM pg_class c
      JOIN pg_namespace n ON n.oid = c.relnamespace
     WHERE n.nspname = 'public'
       AND c.relkind IN ('r', 'p', 'v', 'm', 'f', 'S')
       AND NOT EXISTS (SELECT 1 FROM pg_depend d
                        WHERE d.classid = 'pg_class'::regclass AND d.objid = c.oid AND d.deptype = 'e')
)
SELECT json_build_object(
  'relations', (SELECT coalesce(json_agg(r ORDER BY r.name), '[]'::json) FROM (
    SELECT rel.relname AS name,
           rel.relkind AS kind,
           rel.owner,
           obj_description(rel.oid, 'pg_class') AS comment,
           rel.relrowsecurity AS rls,
           rel.relforcerowsecurity AS force_rls,
           (SELECT pt.relname FROM pg_inherits i JOIN pg_class pt ON pt.oid = i.inhparent
             WHERE i.inhrelid = rel.oid AND (SELECT relispartition FROM pg_class WHERE oid = rel.oid)) AS partition_of,
           pg_get_expr(pc.relpartbound, rel.oid) AS partition_bound,
           CASE WHEN rel.relkind = 'p' THEN pg_get_partkeydef(rel.oid) END AS partition_key,
           (SELECT coalesce(json_agg(json_build_object(
                       'name', a.attname,
                       'type', format_type(a.atttypid, a.atttypmod),
                       'not_null', a.attnotnull,
                       'default', pg_get_expr(ad.adbin, ad.adrelid),
                       'generated', a.attgenerated <> '',
                       'identity', a.attidentity <> '',
                       'comment', col_description(rel.oid, a.attnum)) ORDER BY a.attnum), '[]'::json)
              FROM pg_attribute a
              LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum
             WHERE a.attrelid = rel.oid AND a.attnum > 0 AND NOT a.attisdropped) AS columns,
           (SELECT coalesce(json_agg(json_build_object(
                       'name', co.conname, 'type', co.contype,
                       'def', pg_get_constraintdef(co.oid, true)) ORDER BY co.contype, co.conname), '[]'::json)
              FROM pg_constraint co
             WHERE co.conrelid = rel.oid AND co.contype IN ('c', 'p', 'u', 'f', 'x')
               AND co.conislocal) AS constraints,
           (SELECT coalesce(json_agg(json_build_object(
                       'name', po.polname,
                       'command', po.polcmd,
                       'permissive', po.polpermissive,
                       'roles', (SELECT json_agg(CASE WHEN r = 0 THEN 'PUBLIC' ELSE pg_get_userbyid(r) END ORDER BY 1)
                                   FROM unnest(po.polroles) r),
                       'using', pg_get_expr(po.polqual, po.polrelid, true),
                       'check', pg_get_expr(po.polwithcheck, po.polrelid, true)) ORDER BY po.polname), '[]'::json)
              FROM pg_policy po WHERE po.polrelid = rel.oid) AS policies,
           (SELECT coalesce(json_agg(json_build_object(
                       'grantee', CASE WHEN x.grantee = 0 THEN 'PUBLIC' ELSE pg_get_userbyid(x.grantee) END,
                       'privilege', x.privilege_type,
                       'grantable', x.is_grantable) ORDER BY 1), '[]'::json)
              FROM aclexplode(rel.relacl) x) AS grants
      FROM rel JOIN pg_class pc ON pc.oid = rel.oid
  ) r),
  'extensions', (SELECT coalesce(json_agg(extname ORDER BY extname), '[]'::json)
                   FROM pg_extension WHERE extname <> 'plpgsql')
)
"""


def snapshot(admin_url: str) -> dict:
    """Apply the migrations to a database minted for this run and return its catalog, roles relabelled."""
    tag = f"oml_schema_{os.getpid()}_{time.time_ns() & 0xFFFFFFFF:08x}"
    names = {"owner": f"{tag}_owner", "serving": f"{tag}_serving", "lake writer": f"{tag}_lake"}
    admin, admin_env = with_user(admin_url, None, None)
    created_roles: list[str] = []
    created_db = False
    try:
        for role in names.values():
            psql(admin, admin_env, "-c", f"CREATE ROLE {role} LOGIN PASSWORD '{PASSWORD}'")
            created_roles.append(role)
        psql(admin, admin_env, "-c", f"CREATE DATABASE {tag} OWNER {names['owner']}")
        created_db = True
        owner, owner_env = with_user(admin_url, names["owner"], tag)
        owner_env["PGOPTIONS"] = f"-c oml.serving_role={names['serving']}"
        psql(owner, owner_env, "-c", SQLX_TABLE)
        for path in migration_files():
            single = [] if path.read_text().startswith("-- no-transaction") else ["-1"]
            try:
                psql(owner, owner_env, *single, "-f", str(path))
            except RuntimeError as e:
                raise RuntimeError(f"{path.name} did not apply: {e}") from None
        for stmt in lake_grants():
            psql(owner, owner_env, "-c", stmt.replace("{role}", names["lake writer"]))
        on_db, _ = with_user(admin_url, None, tag)
        raw = json.loads(psql(on_db, admin_env, "-c", CATALOG_SQL))
    finally:
        if created_db:
            psql(admin, admin_env, "-c", f"DROP DATABASE IF EXISTS {tag} WITH (FORCE)")
        for role in created_roles:
            psql(admin, admin_env, "-c", f"DROP ROLE IF EXISTS {role}")
    raw["last_migration"] = migration_files()[-1].name
    return relabel(raw, {v: k for k, v in names.items()})


def relabel(raw: dict, labels: dict[str, str]) -> dict:
    """Replace each minted role name with its label; a role the run did not mint is an error."""

    def label(name: str) -> str:
        if name == "PUBLIC":
            return name
        if name not in labels:
            raise RuntimeError(f"the catalog names role {name!r}, which this run did not mint")
        return labels[name]

    for rel in raw["relations"]:
        rel["owner"] = label(rel["owner"])
        for g in rel["grants"]:
            g["grantee"] = label(g["grantee"])
        for p in rel["policies"]:
            p["roles"] = [label(r) for r in p["roles"] or []]
    return raw


# ---------------------------------------------------------------------------
# The page: a function of the snapshot alone
# ---------------------------------------------------------------------------

KIND = {"r": "table", "p": "partitioned table", "v": "view", "m": "materialized view", "f": "foreign table", "S": "sequence"}
COMMAND = {"r": "SELECT", "a": "INSERT", "w": "UPDATE", "d": "DELETE", "*": "ALL"}
CONSTRAINT = {"p": "primary key", "u": "unique", "f": "foreign key", "x": "exclusion"}


def prose(text: str | None) -> str:
    if not text:
        return ""
    text = SELF_LINK.sub("", text)
    return docgen.strip_handles(" ".join(text.split()))


def cell(text: str) -> str:
    return text.replace("|", "\\|").replace("\n", " ")


def code(text: str) -> str:
    text = " ".join(text.split())
    tick = "``" if "`" in text else "`"
    return f"{tick}{cell(text)}{tick}"


def privileges(rel: dict, role: str) -> str:
    if rel["owner"] == role:
        return "owns"
    held = {g["privilege"] for g in rel["grants"] if g["grantee"] == role}
    if not held:
        return ""
    return ", ".join(sorted(held, key=lambda p: (PRIVILEGE_ORDER.index(p) if p in PRIVILEGE_ORDER else 99, p)))


def tables(snap: dict) -> list[dict]:
    """The relations a heading is written for: every table and view that is not a partition."""
    return sorted((r for r in snap["relations"] if r["kind"] != "S" and not r["partition_of"]), key=lambda r: r["name"])


def render_table(rel: dict, partitions: list[dict]) -> list[str]:
    out = [f"### {rel['name']}", ""]
    about = prose(rel["comment"]) or NOTES.get(rel["name"], "")
    out.append(about or "No comment on the table.")
    out.append("")
    kind = KIND[rel["kind"]]
    if rel["kind"] == "p":
        kind += f", `PARTITION BY {rel['partition_key']}`"
    out += [f"Kind: {kind}.", ""]
    out += ["| Column | Type | Null | Default | Purpose |", "|---|---|---|---|---|"]
    for c in rel["columns"]:
        default = code(c["default"]) if c["default"] else ""
        if c["generated"]:
            default = "generated: " + default
        null = "not null" if c["not_null"] else "null"
        out.append(f"| `{c['name']}` | `{c['type']}` | {null} | {default} | {cell(prose(c['comment']))} |")
    out.append("")
    checks = [c for c in rel["constraints"] if c["type"] == "c"]
    keys = [c for c in rel["constraints"] if c["type"] != "c"]
    if checks:
        out += ["Checks:", ""]
        out += [f"- `{c['name']}`: {code(c['def'])}" for c in checks]
        out.append("")
    if keys:
        out += ["Keys:", ""]
        out += [f"- `{c['name']}` ({CONSTRAINT[c['type']]}): {code(c['def'])}" for c in keys]
        out.append("")
    if rel["kind"] in ("r", "p"):
        if rel["rls"]:
            rls = "enabled and forced, so the owner is bound too" if rel["force_rls"] else "enabled, not forced"
        else:
            rls = "not enabled"
        out += [f"Row-level security: {rls}.", ""]
        for p in rel["policies"]:
            roles = ", ".join(p["roles"]) if p["roles"] else "PUBLIC"
            mode = "permissive" if p["permissive"] else "restrictive"
            out.append(f"- Policy `{p['name']}`, {COMMAND[p['command']]} for {roles}, {mode}:")
            if p["using"]:
                out.append(f"    - using {code(p['using'])}")
            if p["check"]:
                out.append(f"    - with check {code(p['check'])}")
        if rel["policies"]:
            out.append("")
    grants = [f"{role}: {privileges(rel, role)}" for role in ROLES[1:] if privileges(rel, role)]
    public = privileges(rel, "PUBLIC")
    if public:
        grants.append(f"PUBLIC: {public}")
    out += [f"Grants: {'; '.join(grants) if grants else 'none beyond the owner'}.", ""]
    if partitions:
        out += ["Partitions after the last migration:", ""]
        out += [f"- `{p['name']}` {code(p['partition_bound'])}" for p in partitions]
        out.append("")
    elif rel["kind"] == "p":
        out += ["Partitions after the last migration: none. The lead creates one per tenant as the tenant is created.", ""]
    return out


def render(snap: dict) -> str:
    rels = tables(snap)
    by_parent: dict[str, list[dict]] = {}
    for r in snap["relations"]:
        if r["partition_of"]:
            by_parent.setdefault(r["partition_of"], []).append(r)
    sequences = sorted((r for r in snap["relations"] if r["kind"] == "S"), key=lambda r: r["name"])
    out = [
        docgen.front_matter("every PostgreSQL table after the last migration: columns, checks, row-level security, grants per role and partitions").rstrip("\n"),
        "",
        "# PostgreSQL schema",
        "",
        f"This page lists the PostgreSQL schema as the migrations leave it after `{snap['last_migration']}`: "
        "each table's purpose, its columns, checks and keys, its row-level security and policies, "
        "the privileges each role holds, and its partitions. "
        "The [lake tables](delta-tables.md) are a derived copy and have their own page.",
        "",
        f"*{docgen.BANNER.format(script=SCRIPT)} The script applies `migrations/` to a throwaway "
        "database and reads the catalog; purposes are the tables' and columns' `COMMENT`s.*",
        "",
        "## Migrations",
        "",
        "Migrations are forward-only. A file under `migrations/` is never edited once released; a "
        "correction is the next numbered file. The server applies pending files at start, under the "
        "owner role, and records them in `_sqlx_migrations`, which only the owner reaches. A file "
        "whose first line is `-- no-transaction` runs outside the migration transaction.",
        "",
        f"Extensions the migrations create: {', '.join(f'`{e}`' for e in snap['extensions']) or 'none'}. "
        "`pg_trgm` is created only where the owner may create it; without it the opt-in "
        "`bm25-trigram` pipeline is not served.",
        "",
        "## Privileges",
        "",
        "Three roles reach the database. The owner role owns every table and runs the migrations. "
        "The serving role, which the server logs in as, owns nothing and holds only the statements "
        "it runs, granted by the migrations. The lake writer's role is granted by the lake writer "
        "itself at start, through the owner's connection. "
        + ("No table or sequence grants anything to `PUBLIC`." if not any(
            g["grantee"] == "PUBLIC" for r in snap["relations"] for g in r["grants"])
           else "The tables that grant to `PUBLIC` name it under their grants."),
        "",
        "| Table | owner | serving | lake writer |",
        "|---|---|---|---|",
    ]
    for r in rels:
        out.append(f"| [`{r['name']}`](#{r['name']}) | " + " | ".join(privileges(r, role) for role in ROLES) + " |")
    out.append("")
    if sequences:
        out += ["Sequences:", "", "| Sequence | owner | serving | lake writer |", "|---|---|---|---|"]
        for s in sequences:
            out.append(f"| `{s['name']}` | " + " | ".join(privileges(s, role) for role in ROLES) + " |")
        out.append("")
    out += [
        "A partition is reached through its parent and grants nothing of its own. Row-level security "
        "binds every role but a superuser, the owner included where it is forced.",
        "",
        "## Tables",
        "",
    ]
    for r in rels:
        out += render_table(r, sorted(by_parent.get(r["name"], []), key=lambda p: p["name"]))
    return "\n".join(out)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true", help="fail if the page differs from the migrations")
    ap.add_argument("--database-url", default=os.environ.get("OML_SCHEMA_DATABASE_URL") or os.environ.get("DATABASE_URL"),
                    help="a PostgreSQL superuser URL; the run creates and drops its own database and roles")
    args = ap.parse_args(argv)
    if not args.database_url:
        print("gen_postgres_schema.py needs a PostgreSQL superuser URL: --database-url, "
              "OML_SCHEMA_DATABASE_URL or DATABASE_URL", file=sys.stderr)
        return 2
    try:
        snap = snapshot(args.database_url)
    except RuntimeError as e:
        print(f"gen_postgres_schema.py: {e}", file=sys.stderr)
        return 1
    text = render(snap)
    if args.check and OUT.exists() and OUT.read_text() != text.rstrip("\n") + "\n":
        # The job's log carries the difference, so a catalog that renders differently on the CI
        # server can be read and fixed without a local run.
        sys.stderr.writelines(difflib.unified_diff(
            OUT.read_text().splitlines(keepends=True), (text.rstrip("\n") + "\n").splitlines(keepends=True),
            str(OUT.relative_to(docgen.ROOT)), "generated"))
    return docgen.finish(OUT, text, args.check)


if __name__ == "__main__":
    sys.exit(main())
