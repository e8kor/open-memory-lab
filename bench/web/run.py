#!/usr/bin/env python3
"""The web console's per-page overhead over the API (ADR-0070 d8, #170).

One run: a throwaway PostgreSQL cluster, the lead and `oml-web` as their own
release binaries over loopback, a seeded tenant and one signed-in person,
then every page `oml-web` serves to a `GET`, timed many times. Standard
library only.

    python3 bench/web/run.py --bin-dir "$CARGO_TARGET_DIR/release" \\
        --pg-bin /usr/lib/postgresql/18/bin --work .tmp/web-bench \\
        --out bench/web/results/web-pages-6.json

Each page is timed at the client from the request's first byte sent to the
answer's last byte read, over one keep-alive loopback connection to
`oml-web`, so the time is oml-web's request in to response out plus one
loopback exchange; `GET /web/healthz`, which calls nothing, is timed the
same way and gives that floor. The pages are sent in turn, one round of
every page per iteration, so load that drifts during the run spreads over
every page alike.

Each page request carries its own `X-Request-Id`, which `oml-web` honours
and numbers its API calls under (`<id>.<n>`). The lead's request lines name
each call's handler time, its store time and when it ended, so the run
counts the page's API calls, the sequential rounds among them (the longest
chain of calls that did not overlap at the lead) and the time the lead
spent on them.

The lead is built without the reranker (`--no-default-features`) and with
the `e2e-sign-in` feature, so the person signs in through the real routes
with no provider call (crates/bins/oml-server-lead/src/e2e.rs). Nothing
here reads the environment's OML_URL or OML_API_KEY: both processes start
with an environment of their own.
"""

import argparse
import http.client
import json
import os
import platform
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import time
import urllib.parse
from datetime import datetime, timezone
from pathlib import Path

SCHEMA = "oml-web-bench/1"
EMAIL = "bench@example.com"
USER = "bench"


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def percentile(values, p):
    """Nearest-rank percentile, as the recall benchmarks report it."""
    if not values:
        return None
    s = sorted(values)
    k = max(0, min(len(s) - 1, int(-(-p * len(s) // 100)) - 1))
    return s[k]


def summary(values):
    return {
        "n": len(values),
        "p50": round(percentile(values, 50), 3),
        "p95": round(percentile(values, 95), 3),
        "p99": round(percentile(values, 99), 3),
        "max": round(max(values), 3),
    }


class Http:
    """One keep-alive connection to `port` on loopback."""

    def __init__(self, port):
        self.port = port
        self.conn = http.client.HTTPConnection("127.0.0.1", port, timeout=30)

    def send(self, method, path, headers=None, body=None):
        headers = dict(headers or {})
        if body is not None and not isinstance(body, bytes):
            body = body.encode()
        started = time.perf_counter_ns()
        try:
            self.conn.request(method, path, body=body, headers=headers)
            response = self.conn.getresponse()
            data = response.read()
        except (http.client.HTTPException, OSError):
            self.conn.close()
            self.conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=30)
            raise
        elapsed = (time.perf_counter_ns() - started) / 1e6
        return response.status, response.getheaders(), data, elapsed

    def json(self, method, path, bearer=None, body=None, idem=True):
        headers = {"Content-Type": "application/json"}
        if bearer:
            headers["Authorization"] = f"Bearer {bearer}"
        if idem and method == "POST":
            headers["Idempotency-Key"] = secrets.token_hex(12)
        status, _, data, _ = self.send(method, path, headers,
                                       None if body is None else json.dumps(body))
        answer = json.loads(data) if data else None
        if status >= 300:
            raise RuntimeError(f"{method} {path} answered {status}: {data[:400]!r}")
        return status, answer


def header(headers, name):
    return [v for k, v in headers if k.lower() == name]


def set_cookie(headers, name):
    for value in header(headers, "set-cookie"):
        if value.startswith(f"{name}="):
            return value[len(name) + 1:].split(";", 1)[0]
    return None


def wait_for(port, path, seconds=60):
    deadline = time.time() + seconds
    while time.time() < deadline:
        try:
            c = http.client.HTTPConnection("127.0.0.1", port, timeout=2)
            c.request("GET", path)
            if c.getresponse().status == 200:
                return
        except OSError:
            pass
        time.sleep(0.2)
    raise RuntimeError(f"127.0.0.1:{port}{path} never answered 200")


class Postgres:
    """A throwaway cluster under `root`, trust auth on loopback only."""

    def __init__(self, pg_bin, root, port):
        self.bin, self.root, self.port = Path(pg_bin), Path(root), port
        self.data = self.root / "pg"

    def start(self):
        self.root.mkdir(parents=True, exist_ok=True)
        subprocess.run([self.bin / "initdb", "-D", self.data, "-U", "bench", "--auth=trust",
                        "-E", "UTF8", "--no-locale"], check=True, stdout=subprocess.DEVNULL)
        with open(self.data / "postgresql.conf", "a") as f:
            f.write(f"port = {self.port}\nlisten_addresses = '127.0.0.1'\n"
                    f"unix_socket_directories = '{self.root}'\nmax_wal_size = 256MB\n")
        subprocess.run([self.bin / "pg_ctl", "-D", self.data, "-l", self.root / "pg.log", "-w",
                        "start"], check=True, stdout=subprocess.DEVNULL)
        sql = ("CREATE ROLE oml_owner LOGIN PASSWORD 'bench'; "
               "CREATE ROLE oml_server LOGIN PASSWORD 'bench'; "
               "CREATE DATABASE oml OWNER oml_owner;")
        for statement in sql.split("; "):
            subprocess.run([self.bin / "psql", "-h", "127.0.0.1", "-p", str(self.port), "-U",
                            "bench", "-d", "postgres", "-qc", statement], check=True)

    def url(self, role):
        return f"postgres://{role}:bench@127.0.0.1:{self.port}/oml"

    def version(self):
        out = subprocess.run([self.bin / "postgres", "--version"], capture_output=True, text=True)
        return out.stdout.strip()

    def stop(self):
        subprocess.run([self.bin / "pg_ctl", "-D", self.data, "-m", "fast", "-w", "stop"],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


class Stack:
    """The lead and oml-web, each with an environment of its own."""

    def __init__(self, bin_dir, work, pg):
        self.bin_dir, self.work, self.pg = Path(bin_dir), Path(work), pg
        self.lead_port, self.web_port = free_port(), free_port()
        self.issuer = "https://bench.oml.test"
        self.web_secret = secrets.token_hex(32)
        self.procs = []

    def base_env(self):
        return {"PATH": "/usr/bin:/bin", "HOME": str(self.work), "LANG": "C.UTF-8"}

    def start_lead(self, quota):
        env = self.base_env() | {
            "OML_POSTGRES__URL": self.pg.url("oml_server"),
            "OML_POSTGRES__OWNER_URL": self.pg.url("oml_owner"),
            "OML_SERVER__LISTEN": f"127.0.0.1:{self.lead_port}",
            "OML_KMS__KEYRING_PATH": str(self.work / "keyring" / "signing.key"),
            "OML_WEB__CLIENT_SECRET": self.web_secret,
            "OML_OAUTH__ISSUER": self.issuer,
            "OML_WEB__GITHUB__CLIENT_ID": "bench",
            "OML_WEB__GITHUB__CLIENT_SECRET": "bench-double-not-a-secret",
            "OML_E2E_SIGN_IN_EMAIL": EMAIL,
            "OML_QUOTA__REQUESTS_PER_MINUTE": str(quota),
        }
        (self.work / "keyring").mkdir(parents=True, exist_ok=True)
        os.chmod(self.work / "keyring", 0o700)
        self.lead_log = self.work / "lead.log"
        log = open(self.lead_log, "w")
        self.procs.append(subprocess.Popen([self.bin_dir / "oml-server-lead"], env=env,
                                           stdout=log, stderr=subprocess.STDOUT))
        wait_for(self.lead_port, "/healthz")

    def enrollment_token(self):
        lines = self.lead_log.read_text().splitlines()
        for i, line in enumerate(lines):
            if line.startswith("OML bootstrap: empty deployment"):
                return lines[i + 1].strip()
        raise RuntimeError("the lead printed no enrollment token")

    def start_web(self):
        env = self.base_env() | {
            "OML_WEB__API_URL": f"http://127.0.0.1:{self.lead_port}",
            "OML_WEB__ALLOW_PLAIN_HTTP_API": "true",
            "OML_WEB__LISTEN": f"127.0.0.1:{self.web_port}",
            "OML_WEB__CLIENT_SECRET": self.web_secret,
            "OML_WEB__PUBLIC_URL": self.issuer,
        }
        self.web_log = self.work / "web.log"
        log = open(self.web_log, "w")
        self.procs.append(subprocess.Popen([self.bin_dir / "oml-web"], env=env, stdout=log,
                                           stderr=subprocess.STDOUT))
        wait_for(self.web_port, "/web/healthz")

    def stop(self):
        for p in reversed(self.procs):
            p.send_signal(signal.SIGTERM)
        for p in self.procs:
            try:
                p.wait(timeout=20)
            except subprocess.TimeoutExpired:
                p.kill()


def sign_in(lead, issuer):
    """GitHub sign-in through the lead's real routes: the session token."""
    status, headers, data, _ = lead.send(
        "POST", "/web/sign-in",
        {"Origin": issuer, "Content-Type": "application/x-www-form-urlencoded"},
        "provider=github")
    if status != 303:
        raise RuntimeError(f"POST /web/sign-in answered {status}: {data[:300]!r}")
    location = header(headers, "location")[0]
    state = urllib.parse.parse_qs(urllib.parse.urlsplit(location).query)["state"][0]
    sealed = set_cookie(headers, "__Host-oml_signin")
    status, headers, data, _ = lead.send(
        "GET", f"/web/callback/github?code=bench&state={urllib.parse.quote(state)}",
        {"Cookie": f"__Host-oml_signin={sealed}"})
    session = set_cookie(headers, "__Host-oml_session")
    if status != 303 or not session:
        raise RuntimeError(f"the callback answered {status} and no session: {data[:300]!r}")
    return session


def seed(lead, admin, workareas, memories):
    """Workareas under the person's root, memories written by a user agent
    key and two thirds of them approved by two human reviewers in turn."""
    root = USER
    names = [root]
    for i in range(1, workareas):
        _, node = lead.json("POST", "/v2/workareas", admin, {"parent": root, "name": f"p{i}"})
        names.append(node["id"])
    _, writer = lead.json("POST", "/v2/keys", admin, {
        "role": "writer", "name": f"{USER}-bench-writer", "user": USER, "held_by": "agent",
        "principal": "bench/writer", "default_workarea": root,
        "grants": [{"workarea": root, "role": "writer"}]})
    reviewers = []
    for side in ("a", "b"):
        _, key = lead.json("POST", "/v2/keys", admin, {
            "role": "dreamer", "name": f"bench-reviewer-{side}", "held_by": "human",
            "principal": f"bench/reviewer-{side}", "default_workarea": root,
            "grants": [{"workarea": root, "role": "dreamer"}]})
        reviewers.append(key["value"])
    ids = []
    for n in range(memories):
        _, receipt = lead.json("POST", "/v1/memories", writer["value"], {
            "body": f"Bench memory {n}: the service in area {n % workareas} answers "
                    f"within its budget and logs one line per request {n}.",
            "workarea": names[n % workareas]})
        ids.append(receipt["artifact_id"])
    approved = 0
    for n, artifact in enumerate(ids):
        if n % 3 == 2:
            continue
        reviewer = reviewers[approved % 2]
        status, served = lead.json("POST", "/v2/review/next", reviewer, {})
        if status == 204:
            raise RuntimeError("the review queue was empty")
        lead.json("POST", "/v2/review/verdicts", reviewer,
                  {"verdict": "approve", "serve_id": served["serve_id"]})
        approved += 1
    return {"workareas": len(names), "memories": memories, "approved": approved,
            "held": memories - approved}, ids


def pages(promoted_id, in_process=False):
    """Every GET page oml-web serves to a signed-in admin, as (label, path).
    In process, each page's URL before phase (b) moved it, where the lead
    served one; the admin keys page had none, and the admin Review view was
    the queue and the held list, two pages."""
    if in_process:
        return [
            ("guardrails", "/web?view=guardrails"),
            ("workareas", "/web?view=workarea"),
            ("admin/workareas", "/web/admin?view=workarea"),
            ("memories", "/web?view=memories"),
            ("memories?q", "/web?view=memories&q=budget+service"),
            ("atlas", "/web?view=memories&display=atlas"),
            ("data/memories", "/web/data/memories"),
            ("admin/memories", "/web/admin?view=memories"),
            ("admin/atlas", "/web/admin?view=memories&display=atlas"),
            ("admin/data/memories", "/web/admin/data/memories"),
            ("memory", f"/web?view=memories&memory={promoted_id}"),
            ("admin/memory", f"/web/admin?view=memories&memory={promoted_id}"),
            ("admin/curate", "/web/admin?view=curate"),
            ("review", "/web?view=review"),
            ("admin/review (held)", "/web/admin?view=held"),
            ("admin/review (queue)", "/web/admin?view=queue"),
            ("keys", "/web?view=keys"),
            ("start", "/web/start"),
            ("start/status", "/web/start/status"),
            ("start/export", "/web/start/export"),
            ("setup", "/web?view=setup"),
            ("admin/people", "/web/admin?view=people"),
            ("admin/users", "/web/admin/users"),
        ]
    return [
        ("guardrails", "/web/guardrails"),
        ("workareas", "/web/workareas"),
        ("admin/workareas", "/web/admin/workareas"),
        ("memories", "/web/memories"),
        ("memories?q", "/web/memories?q=budget+service"),
        ("atlas", "/web/atlas"),
        ("data/memories", "/web/data/memories"),
        ("admin/memories", "/web/admin/memories"),
        ("admin/atlas", "/web/admin/atlas"),
        ("admin/data/memories", "/web/admin/data/memories"),
        ("memory", f"/web/memory?id={promoted_id}"),
        ("admin/memory", f"/web/admin/memory?id={promoted_id}"),
        ("admin/curate", "/web/admin/curate"),
        ("review", "/web/review"),
        ("admin/review", "/web/admin/review"),
        ("keys", "/web/keys"),
        ("admin/keys", f"/web/admin/keys?user={USER}"),
        ("start", "/web/start"),
        ("start/status", "/web/start/status"),
        ("start/export", "/web/start/export"),
        ("setup", "/web/setup"),
        ("admin/people", "/web/admin/people"),
        ("admin/users", "/web/admin/users"),
    ]


def measure(web, session, page_list, iterations, warmup, floor_path):
    """Each page in turn per iteration: (label -> [(ms, request id)]), the
    healthz floor, and the statuses seen."""
    cookie = {"Cookie": f"__Host-oml_session={session}"}
    times = {label: [] for label, _ in page_list}
    floor = []
    statuses = {}
    seq = 0
    for i in range(warmup + iterations):
        for label, path in page_list:
            seq += 1
            rid = f"wb{seq}"
            status, _, data, ms = web.send("GET", path, cookie | {"X-Request-Id": rid})
            statuses.setdefault(label, set()).add(status)
            if status != 200:
                raise RuntimeError(f"GET {path} answered {status}: {data[:300]!r}")
            if i >= warmup:
                times[label].append((ms, rid))
        _, _, _, ms = web.send("GET", floor_path)
        if i >= warmup:
            floor.append(ms)
    return times, floor, statuses


def lead_calls(lead_log):
    """The lead's request lines by request id: (end µs, handler ms, store ms,
    store calls, route, key name)."""
    calls = {}
    with open(lead_log) as f:
        for line in f:
            if not line.startswith("{"):
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            rid = row.get("request_id")
            if row.get("event") != "http_request" or not rid:
                continue
            end = datetime.fromisoformat(row["timestamp"].replace("Z", "+00:00"))
            calls[rid] = (end.timestamp() * 1e6, float(row["duration_ms"]),
                          float(row.get("store_ms") or 0), int(row.get("store_calls") or 0),
                          f"{row.get('method')} {row.get('route')}", row.get("key_name"))
    return calls


def rounds(intervals):
    """The longest chain of intervals each starting after the one before it
    ended: the sequential API rounds a page waited on."""
    intervals = sorted(intervals)
    best = []
    for i, (start, _end) in enumerate(intervals):
        best.append(1 + max((best[j] for j in range(i) if intervals[j][1] <= start), default=0))
    return max(best, default=0)


def union_ms(intervals):
    total, cur_s, cur_e = 0.0, None, None
    for s, e in sorted(intervals):
        if cur_e is None or s > cur_e:
            if cur_e is not None:
                total += cur_e - cur_s
            cur_s, cur_e = s, e
        else:
            cur_e = max(cur_e, e)
    if cur_e is not None:
        total += cur_e - cur_s
    return total / 1000


def analyse(times, floor, calls, in_process):
    """Per page: its API calls joined by request id. In process the lead's
    own line for the page stands in for them, and no API call is counted."""
    by_id = {}
    for rid, row in calls.items():
        head, _, n = rid.rpartition(".")
        if in_process and rid.startswith("wb") and "." not in rid:
            by_id[rid] = [row]
        elif not in_process and head.startswith("wb") and n.isdigit():
            by_id.setdefault(head, []).append(row)
    out = {}
    for label, samples in times.items():
        counts, depth, lead_sum, lead_union, store, rest = [], [], [], [], [], []
        routes, keys, route_ms, route_store = {}, {}, {}, {}
        for ms, rid in samples:
            rows = by_id.get(rid, [])
            spans = [(end - dur * 1000, end) for end, dur, *_ in rows]
            counts.append(0 if in_process else len(rows))
            depth.append(0 if in_process else rounds(spans))
            lead_sum.append(sum(r[1] for r in rows))
            lead_union.append(union_ms(spans))
            store.append(sum(r[2] for r in rows))
            rest.append(ms - union_ms(spans))
            for r in rows:
                routes[r[4]] = routes.get(r[4], 0) + 1
                keys[r[5]] = keys.get(r[5], 0) + 1
                route_ms.setdefault(r[4], []).append(r[1])
                route_store.setdefault(r[4], []).append(r[3])
        n = len(samples)
        out[label] = {
            "page_ms": summary([s[0] for s in samples]),
            "api_calls": {"min": min(counts), "max": max(counts),
                          "mean": round(sum(counts) / n, 2)},
            "sequential_rounds": {"min": min(depth), "max": max(depth),
                                  "p50": percentile(depth, 50)},
            "lead_handler_ms_sum": summary(lead_sum),
            "lead_busy_ms": summary(lead_union),
            "store_ms_sum": summary(store),
            "outside_lead_ms": summary(rest),
            "calls_by_route": {k: round(v / n, 2) for k, v in sorted(routes.items())},
            "lead_ms_by_route": {k: {"p50": round(percentile(v, 50), 3),
                                     "p95": round(percentile(v, 95), 3),
                                     "store_calls_p50": percentile(route_store[k], 50)}
                                 for k, v in sorted(route_ms.items())},
            "calls_by_key": {} if in_process else
            {str(k): round(v / n, 2) for k, v in sorted(keys.items(), key=lambda kv: str(kv[0]))},
        }
    return out, summary(floor)


def loadavg():
    try:
        return [round(x, 2) for x in os.getloadavg()]
    except OSError:
        return None


def git(*args):
    try:
        return subprocess.run(["git", *args], capture_output=True, text=True,
                              check=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--bin-dir", required=True, help="where oml-server-lead and oml-web are")
    ap.add_argument("--pg-bin", required=True, help="PostgreSQL's bin directory (initdb, pg_ctl)")
    ap.add_argument("--work", required=True, help="a scratch directory, removed at the end")
    ap.add_argument("--out", required=True, help="the result file")
    ap.add_argument("--workareas", type=int, default=6, help="workareas in the person's root")
    ap.add_argument("--memories", type=int, default=240)
    ap.add_argument("--iterations", type=int, default=500)
    ap.add_argument("--warmup", type=int, default=50)
    ap.add_argument("--quota", type=int, default=1_000_000,
                    help="quota.requests_per_minute, high so the run never meets limit.rate")
    ap.add_argument("--in-process", action="store_true",
                    help="a lead from before phase (b), which serves the pages itself; "
                         "oml-web is not started")
    ap.add_argument("--commit", help="the commit the binaries were built from; "
                                     "default this checkout's HEAD")
    ap.add_argument("--keep", action="store_true", help="keep the scratch directory")
    a = ap.parse_args()

    work = Path(a.work).resolve()
    if work.exists():
        sys.exit(f"{work} exists; the run makes its own scratch directory")
    pg = Postgres(a.pg_bin, work, free_port())
    stack = Stack(a.bin_dir, work, pg)
    try:
        pg.start()
        stack.start_lead(a.quota)
        if not a.in_process:
            stack.start_web()
        lead = Http(stack.lead_port)
        _, claimed = lead.json("POST", "/v2/bootstrap/claim", None,
                               {"enrollment_token": stack.enrollment_token(),
                                "name": "root-admin"}, idem=False)
        admin = claimed["value"]
        lead.json("POST", "/v2/web/emails", admin, {"email": EMAIL, "user": USER, "admin": True})
        session = sign_in(lead, stack.issuer)
        seeded, ids = seed(lead, admin, a.workareas, a.memories)
        load_before = loadavg()
        web = Http(stack.lead_port if a.in_process else stack.web_port)
        page_list = pages(ids[0], a.in_process)
        started = time.time()
        times, floor, _ = measure(web, session, page_list, a.iterations, a.warmup,
                                  "/healthz" if a.in_process else "/web/healthz")
        seconds = time.time() - started
        load_after = loadavg()
        time.sleep(0.5)
        result, floor_summary = analyse(times, floor, lead_calls(stack.lead_log), a.in_process)
    finally:
        stack.stop()
        pg.stop()
    doc = {
        "schema": SCHEMA,
        "suite": "web-pages",
        "shape": "in-process" if a.in_process else "oml-web",
        "workareas": a.workareas,
        "date": datetime.now(timezone.utc).strftime("%Y-%m-%d"),
        "commit": a.commit or git("rev-parse", "--short=12", "HEAD"),
        "environment": {
            "host": platform.node(), "cpus": os.cpu_count(), "kernel": platform.release(),
            "python": platform.python_version(), "postgres": pg.version(),
            "loadavg_before": load_before, "loadavg_after": load_after,
            "build": "release, debuginfo off; oml-server-lead --no-default-features "
                     "--features e2e-sign-in",
        },
        "method": {
            "iterations": a.iterations, "warmup": a.warmup, "seconds": round(seconds, 1),
            "transport": "loopback, HTTP/1.1 keep-alive; client to oml-web, oml-web to lead",
            "timing": "client wall time per GET over one keep-alive loopback connection to "
                      "the page's server (oml-web, or the lead in process); healthz_ms is the same "
                      "exchange for that server's health probe, which calls nothing",
            "percentiles": "nearest rank",
        },
        "seeded": seeded,
        "healthz_ms": floor_summary,
        "pages": result,
    }
    Path(a.out).parent.mkdir(parents=True, exist_ok=True)
    Path(a.out).write_text(json.dumps(doc, indent=1) + "\n")
    print(f"wrote {a.out}: {len(result)} pages, {a.iterations} iterations in {seconds:.0f} s")
    if not a.keep:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
