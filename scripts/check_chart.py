"""The Helm chart renders the one lead ADR-0033 decides and oml-web beside it, at the workspace version.

Run by .github/workflows/ci.yml (job chart). Renders deploy/helm/oml with
`helm template` and checks, failing with every disagreement found:

- Chart.yaml's version and appVersion are the workspace version (Cargo.toml),
  so one tag names the binaries, the image, the plugin and the chart (d9);
- `helm lint --strict` passes with the default values and with each
  ci/*-values.yaml file;
- the default image is ghcr.io/e8kor/open-memory-lab:<appVersion>, and
  image.digest pins it as <repository>:<tag>@<digest> (d9);
- exactly one lead Deployment, at one replica, with the Recreate strategy (d2);
- exactly one oml-web Deployment (ADR-0070 d4), at one replica, from the same
  image under the command /usr/local/bin/oml-web, its pods outside the
  server selector, hardened as the lead's, reaching the lead's Service
  through OML_WEB__API_URL, with startup and liveness on GET /web/healthz
  and readiness on GET /readyz on its http port, a ClusterIP Service of its
  own, and OML_WEB__CLIENT_SECRET from the same Secret and key as the lead's
  (ADR-0070 d2); with the NetworkPolicy on, oml-web's pods admit their http
  port alone and reach DNS and the lead's pods alone, and the lead admits
  them;
- the pod sets runAsNonRoot, a numeric non-root runAsUser and runAsGroup,
  fsGroup with fsGroupChangePolicy OnRootMismatch (under Always the kubelet
  ORs 0660 into the 0600 keyring and the lead refuses it) and seccomp
  RuntimeDefault; every container sets readOnlyRootFilesystem, no privilege
  escalation, drops ALL capabilities and adds none (d4, d5);
- the ServiceAccount and the pod both refuse the token automount (d5);
- the startup and liveness probes GET /healthz and the readiness probe GET
  /readyz, on the container port the Dockerfile's OML_SERVER__LISTEN names
  (d7);
- the keyring is a ReadWriteOnce claim that helm uninstall keeps, mounted at
  the directory of the Dockerfile's OML_KMS__KEYRING_PATH; with
  keyring.existingClaim no claim is rendered and the pod mounts the named one
  (d3);
- OML_POSTGRES__URL comes from a secretKeyRef, and no env value is a
  postgres:// URL (d6);
- OML_POSTGRES__OWNER_URL, the owner login the lead migrates under, comes
  from a secretKeyRef in the same Secret as OML_POSTGRES__URL (ADR-0033 d6,
  ADR-0034 d4, d10 step 2);
- values.yaml sets no resources and the default render requests and limits
  nothing: resource hints are comments, never claims (d5);
- the Service is a ClusterIP that selects the lead's pods by its server
  component label; by default there is no Ingress and no NetworkPolicy; when
  enabled, the Ingress routes to the Service's http port, and the
  NetworkPolicy selects the server pods, admits the API port alone and lets
  them reach DNS and the Postgres port alone (d5, d8);
- NOTES.txt names the port-forward, `oml bootstrap claim` and the Ingress
  upgrade, and warns when an install enables the Ingress (d8).

With --image-user UID:GID (the built image's `id -u` and `id -g`), it also
checks that the pod runs as the image's own user.

Dependencies: stdlib + PyYAML, and helm on PATH.
"""

import argparse
import posixpath
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent
CHART = ROOT / "deploy/helm/oml"
DOCKERFILE = ROOT / "Dockerfile"
IMAGE_REPOSITORY = "ghcr.io/e8kor/open-memory-lab"
RELEASE = "oml"
NAMESPACE = "oml"
DIGEST = "sha256:" + "0" * 64
KEPT_CLAIM = "kept-keyring"
# The Ingress override NOTES.txt and docs/operations/deployment.md print, host placeholder filled.
INGRESS_HOST_FLAGS = (
    "--set", "ingress.enabled=true",
    "--set", "ingress.hosts[0].host=oml.example",
    "--set", "ingress.hosts[0].paths[0].path=/",
    "--set", "ingress.hosts[0].paths[0].pathType=Prefix",
)
SERVER_COMPONENT = ("app.kubernetes.io/component", "server")
LEAD_ROLE = ("oml.e8kor.github.io/role", "lead")
PROBES = {"startupProbe": "/healthz", "livenessProbe": "/healthz", "readinessProbe": "/readyz"}
WEB_COMPONENT = ("app.kubernetes.io/component", "web")
WEB_PROBES = {"startupProbe": "/web/healthz", "livenessProbe": "/web/healthz", "readinessProbe": "/readyz"}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--image-user", metavar="UID:GID", help="the built image's id -u and id -g")
    args = parser.parse_args()

    errors: list[str] = []
    if shutil.which("helm") is None:
        errors.append("helm is not on PATH")
        return report(errors)
    if not (CHART / "Chart.yaml").is_file():
        errors.append(f"{CHART.relative_to(ROOT)}/Chart.yaml is missing")
        return report(errors)

    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    chart = yaml.safe_load((CHART / "Chart.yaml").read_text())
    for field in ("version", "appVersion"):
        if str(chart.get(field)) != workspace:
            errors.append(f"Chart.yaml {field} {chart.get(field)!r} != workspace version {workspace}")
    app_version = str(chart.get("appVersion"))

    listen_port, keyring_dir = dockerfile_facts(errors)

    values = yaml.safe_load((CHART / "values.yaml").read_text()) or {}
    if values.get("resources"):
        errors.append("values.yaml sets resources: hints belong in comments, never values (ADR-0033 d5)")

    ci_values = sorted((CHART / "ci").glob("*-values.yaml"))
    if not ci_values:
        errors.append("deploy/helm/oml/ci/ holds no *-values.yaml file for CI")
    for extra in [[]] + [["--values", str(f)] for f in ci_values]:
        lint = helm("lint", "--strict", str(CHART), *extra)
        if lint.returncode != 0:
            errors.append(f"helm lint --strict {' '.join(extra) or '(default values)'} failed:\n{lint.stdout}{lint.stderr}")

    renders = {"default values": render(errors)}
    for f in ci_values:
        renders[f.name] = render(errors, "--values", str(f))
    for label, docs in renders.items():
        if docs is None:
            continue
        check_lead(errors, label, docs, listen_port, keyring_dir, args.image_user)
        check_web(errors, label, docs, args.image_user)
        image = lead_deployment(errors, label, docs)
        if image is not None and label == "default values":
            want = f"{IMAGE_REPOSITORY}:{app_version}"
            got = containers(image)[0].get("image")
            if got != want:
                errors.append(f"{label}: the image is {got!r}, not {want!r}")

    defaults = renders["default values"]
    if defaults is not None:
        for kind in ("Ingress", "NetworkPolicy"):
            if of_kind(defaults, kind):
                errors.append(f"default values: render a {kind}; it is off until asked for (ADR-0033 d5, d8)")
        for deployment in of_kind(defaults, "Deployment"):
            for c in containers(deployment):
                if c.get("resources"):
                    errors.append(f"default values: container {c.get('name')} requests or limits resources")

    options = render(
        errors,
        "--set", "ingress.enabled=true",
        "--set", "networkPolicy.enabled=true",
        "--set", f"image.digest={DIGEST}",
        "--set", f"keyring.existingClaim={KEPT_CLAIM}",
    )
    if options is not None:
        check_options(errors, options, app_version)

    # The host override the install notes print replaces the whole hosts list,
    # so it must carry its path or the Ingress routes nothing.
    hosted = render(errors, *INGRESS_HOST_FLAGS)
    if hosted is not None:
        ingress = only(errors, "the notes' host override", hosted, "Ingress")
        paths = [] if ingress is None else [
            p for rule in ingress.get("spec", {}).get("rules") or [] for p in (rule.get("http") or {}).get("paths") or []
        ]
        if ingress is not None and not paths:
            errors.append("the notes' host override: the Ingress rule has no paths and routes nothing")

    check_notes(errors)
    return report(errors)


def dockerfile_facts(errors: list[str]) -> tuple[int | None, str | None]:
    """The final stage's listen port and keyring directory, from its ENV lines."""
    text = DOCKERFILE.read_text().replace("\\\n", " ")
    final = text.split("\nFROM ")[-1]
    env = " ".join(line[4:] for line in final.splitlines() if line.startswith("ENV "))
    listen = re.search(r"\bOML_SERVER__LISTEN=\S*:(\d+)\b", env)
    keyring = re.search(r"\bOML_KMS__KEYRING_PATH=(\S+)", env)
    if not listen:
        errors.append("Dockerfile: the final stage sets no OML_SERVER__LISTEN with a port")
    if not keyring:
        errors.append("Dockerfile: the final stage sets no OML_KMS__KEYRING_PATH")
    return (
        int(listen.group(1)) if listen else None,
        posixpath.dirname(keyring.group(1)) if keyring else None,
    )


def check_lead(
    errors: list[str],
    label: str,
    docs: list[dict],
    listen_port: int | None,
    keyring_dir: str | None,
    image_user: str | None,
) -> None:
    deployment = lead_deployment(errors, label, docs)
    if deployment is None:
        return
    spec = deployment.get("spec", {})
    if spec.get("replicas") != 1:
        errors.append(f"{label}: the Deployment runs {spec.get('replicas')!r} replicas, not one lead (ADR-0033 d2)")
    if spec.get("strategy", {}).get("type") != "Recreate":
        errors.append(f"{label}: the Deployment's strategy is {spec.get('strategy')!r}, not Recreate (ADR-0033 d2)")
    template_labels = spec.get("template", {}).get("metadata", {}).get("labels", {})
    selector = spec.get("selector", {}).get("matchLabels", {})
    if not selector or selector.items() - template_labels.items():
        errors.append(f"{label}: the Deployment's selector {selector!r} does not match its pod template's labels")
    for key, value in (SERVER_COMPONENT, LEAD_ROLE):
        if template_labels.get(key) != value:
            errors.append(f"{label}: the lead's pods do not carry {key}: {value} (ADR-0033 d1)")

    pod = spec.get("template", {}).get("spec", {})
    psc = pod.get("securityContext", {})
    if psc.get("runAsNonRoot") is not True:
        errors.append(f"{label}: the pod does not set runAsNonRoot")
    for field in ("runAsUser", "runAsGroup", "fsGroup"):
        if not isinstance(psc.get(field), int) or psc.get(field) == 0:
            errors.append(f"{label}: the pod's {field} is {psc.get(field)!r}, not a numeric non-root id (ADR-0033 d4)")
    if psc.get("fsGroupChangePolicy") != "OnRootMismatch":
        errors.append(
            f"{label}: fsGroupChangePolicy is {psc.get('fsGroupChangePolicy')!r}; under Always the kubelet "
            "ORs 0660 into the 0600 keyring and the lead refuses it at its next boot (ADR-0033 d4)"
        )
    if psc.get("seccompProfile", {}).get("type") != "RuntimeDefault":
        errors.append(f"{label}: the pod's seccomp profile is not RuntimeDefault")
    if image_user is not None and f"{psc.get('runAsUser')}:{psc.get('runAsGroup')}" != image_user:
        errors.append(
            f"{label}: the pod runs as {psc.get('runAsUser')}:{psc.get('runAsGroup')}, the image's user is {image_user}"
        )
    if pod.get("automountServiceAccountToken") is not False:
        errors.append(f"{label}: the pod mounts a service account token (ADR-0033 d5)")

    accounts = {d["metadata"]["name"]: d for d in of_kind(docs, "ServiceAccount")}
    account = accounts.get(pod.get("serviceAccountName", ""))
    if account is None:
        errors.append(f"{label}: the pod's service account {pod.get('serviceAccountName')!r} is not rendered")
    elif account.get("automountServiceAccountToken") is not False:
        errors.append(f"{label}: the ServiceAccount does not refuse the token automount (ADR-0033 d5)")

    cs = containers(deployment)
    if len(cs) != 1:
        errors.append(f"{label}: the pod runs {len(cs)} containers, not the one lead")
    for c in cs + pod.get("initContainers", []):
        name = c.get("name")
        sc = c.get("securityContext", {})
        if sc.get("readOnlyRootFilesystem") is not True:
            errors.append(f"{label}: container {name} has a writable root filesystem (ADR-0033 d5)")
        if sc.get("allowPrivilegeEscalation") is not False:
            errors.append(f"{label}: container {name} allows privilege escalation")
        if sc.get("privileged") or sc.get("runAsNonRoot") is False:
            errors.append(f"{label}: container {name} loosens the pod's non-root, unprivileged context")
        caps = sc.get("capabilities", {})
        if caps.get("drop") != ["ALL"] or caps.get("add"):
            errors.append(f"{label}: container {name} capabilities are {caps!r}, not drop [ALL] and add none")
        if sc.get("seccompProfile", {}).get("type", "RuntimeDefault") != "RuntimeDefault":
            errors.append(f"{label}: container {name} overrides the RuntimeDefault seccomp profile")
        for env in c.get("env", []):
            if "postgres://" in str(env.get("value", "")):
                errors.append(f"{label}: container {name} carries a Postgres URL as a literal env value (ADR-0033 d6)")
    if not cs:
        return
    lead = cs[0]

    ports = {p.get("name"): p.get("containerPort") for p in lead.get("ports", [])}
    if listen_port is not None and ports.get("http") != listen_port:
        errors.append(f"{label}: the http container port is {ports.get('http')!r}, the image listens on {listen_port}")
    for probe, path in PROBES.items():
        get = lead.get(probe, {}).get("httpGet", {})
        port = get.get("port")
        if get.get("path") != path or (port != "http" and port != listen_port):
            errors.append(f"{label}: {probe} is {lead.get(probe)!r}, not GET {path} on the http port (ADR-0033 d7)")

    url = next((e for e in lead.get("env", []) if e.get("name") == "OML_POSTGRES__URL"), None)
    if url is None or "secretKeyRef" not in url.get("valueFrom", {}):
        errors.append(f"{label}: OML_POSTGRES__URL does not come from a secretKeyRef (ADR-0033 d6)")
    owner = next((e for e in lead.get("env", []) if e.get("name") == "OML_POSTGRES__OWNER_URL"), None)
    owner_ref = (owner or {}).get("valueFrom", {}).get("secretKeyRef")
    if owner_ref is None:
        errors.append(
            f"{label}: OML_POSTGRES__OWNER_URL does not come from a secretKeyRef; the lead "
            "migrates under the owner URL and refuses to boot without it (ADR-0033 d6, ADR-0034 d4, d5)"
        )
    elif url is not None and owner_ref.get("name") != url.get("valueFrom", {}).get("secretKeyRef", {}).get("name"):
        errors.append(
            f"{label}: OML_POSTGRES__OWNER_URL comes from Secret {owner_ref.get('name')!r}, "
            "not the Secret that holds OML_POSTGRES__URL (ADR-0034 d10 step 2)"
        )

    mounts = {m.get("name"): m.get("mountPath") for m in lead.get("volumeMounts", [])}
    volumes = {v.get("name"): v for v in pod.get("volumes", [])}
    keyring = [n for n, v in volumes.items() if "persistentVolumeClaim" in v]
    if len(keyring) != 1:
        errors.append(f"{label}: the pod mounts {len(keyring)} volume claims, not the one keyring claim (ADR-0033 d3)")
        return
    if keyring_dir is not None and mounts.get(keyring[0]) != keyring_dir:
        errors.append(
            f"{label}: the keyring claim is mounted at {mounts.get(keyring[0])!r}, the image keeps its keyring in {keyring_dir}"
        )
    claim_name = volumes[keyring[0]]["persistentVolumeClaim"].get("claimName")
    claims = {d["metadata"]["name"]: d for d in of_kind(docs, "PersistentVolumeClaim")}
    claim = claims.get(claim_name)
    if claim is None:
        errors.append(f"{label}: the pod mounts claim {claim_name!r}, which the chart does not render")
    else:
        if claim.get("spec", {}).get("accessModes") != ["ReadWriteOnce"]:
            errors.append(f"{label}: the keyring claim's access modes are not [ReadWriteOnce] (ADR-0033 d3)")
        if claim.get("metadata", {}).get("annotations", {}).get("helm.sh/resource-policy") != "keep":
            errors.append(f"{label}: helm uninstall would delete the keyring claim and the signing key with it (ADR-0033 d3)")

    service = lead_service(errors, label, docs)
    if service is not None:
        sspec = service.get("spec", {})
        if sspec.get("type", "ClusterIP") != "ClusterIP":
            errors.append(f"{label}: the Service is a {sspec.get('type')}, not cluster-internal (ADR-0033 d8)")
        selector = sspec.get("selector", {})
        if not selector or any(template_labels.get(k) != v for k, v in selector.items()):
            errors.append(f"{label}: the Service selector {selector!r} does not select the lead's pods")
        if dict([SERVER_COMPONENT]).items() - selector.items() or LEAD_ROLE[0] in selector:
            errors.append(f"{label}: the Service does not select by the server component alone (ADR-0033 d1)")
        if not any(p.get("name") == "http" and p.get("targetPort") in ("http", listen_port) for p in sspec.get("ports", [])):
            errors.append(f"{label}: the Service has no http port targeting the lead's http port")


def check_options(errors: list[str], docs: list[dict], app_version: str) -> None:
    label = "ingress, NetworkPolicy, digest and existing claim enabled"
    deployment = lead_deployment(errors, label, docs)
    service = lead_service(errors, label, docs)
    if deployment is None or service is None:
        return
    want = f"{IMAGE_REPOSITORY}:{app_version}@{DIGEST}"
    got = containers(deployment)[0].get("image")
    if got != want:
        errors.append(f"{label}: the image is {got!r}, not {want!r} (ADR-0033 d9)")

    if of_kind(docs, "PersistentVolumeClaim"):
        errors.append(f"{label}: a keyring claim is rendered although keyring.existingClaim names one")
    claims = [
        v["persistentVolumeClaim"].get("claimName")
        for v in deployment["spec"]["template"]["spec"].get("volumes", [])
        if "persistentVolumeClaim" in v
    ]
    if claims != [KEPT_CLAIM]:
        errors.append(f"{label}: the pod mounts {claims!r}, not the existing claim {KEPT_CLAIM!r}")

    service_name = service["metadata"]["name"]
    ingress = only(errors, label, docs, "Ingress")
    if ingress is not None:
        backends = [
            p.get("backend", {}).get("service", {})
            for rule in ingress.get("spec", {}).get("rules", [])
            for p in rule.get("http", {}).get("paths", [])
        ]
        if not backends or any(b.get("name") != service_name or b.get("port") != {"name": "http"} for b in backends):
            errors.append(f"{label}: the Ingress does not route every path to {service_name}'s http port: {backends!r}")

    policies = [
        p for p in of_kind(docs, "NetworkPolicy")
        if p.get("spec", {}).get("podSelector", {}).get("matchLabels", {}).get(SERVER_COMPONENT[0]) == SERVER_COMPONENT[1]
    ]
    if len(policies) != 1:
        errors.append(f"{label}: renders {len(policies)} NetworkPolicy objects for the server pods, not one")
        return
    check_web_policy(errors, label, docs)
    pspec = policies[0].get("spec", {})
    selected = pspec.get("podSelector", {}).get("matchLabels", {})
    if dict([SERVER_COMPONENT]).items() - selected.items():
        errors.append(f"{label}: the NetworkPolicy does not select the server pods: {selected!r}")
    if sorted(pspec.get("policyTypes", [])) != ["Egress", "Ingress"]:
        errors.append(f"{label}: the NetworkPolicy does not govern both Ingress and Egress")
    in_ports = [p for rule in pspec.get("ingress", []) for p in rule.get("ports", [])]
    if not any(
        (peer.get("podSelector") or {}).get("matchLabels", {}).get(SERVER_COMPONENT[0]) == WEB_COMPONENT[1]
        for rule in pspec.get("ingress", [])
        for peer in rule.get("from", [])
    ):
        errors.append(f"{label}: the lead's NetworkPolicy does not admit oml-web's pods, which reach it at its Service")
    if not pspec.get("ingress") or any(not r.get("ports") for r in pspec["ingress"]) or any(
        p.get("port") != "http" for p in in_ports
    ):
        errors.append(f"{label}: the NetworkPolicy admits more than the API port: {pspec.get('ingress')!r}")
    out_ports = sorted({p.get("port") for rule in pspec.get("egress", []) for p in rule.get("ports", [])}, key=str)
    if not pspec.get("egress") or any(not r.get("ports") for r in pspec["egress"]) or out_ports != sorted({53, 5432}, key=str):
        errors.append(f"{label}: the NetworkPolicy's egress is {pspec.get('egress')!r}, not DNS and the Postgres port alone")


def lead_deployment(errors: list[str], label: str, docs: list[dict]) -> dict | None:
    """The one Deployment whose pods carry the lead role."""
    found = [d for d in of_kind(docs, "Deployment") if pod_labels(d).get(LEAD_ROLE[0]) == LEAD_ROLE[1]]
    if len(found) != 1:
        errors.append(f"{label}: renders {len(found)} lead Deployments, not one")
        return None
    return found[0]


def web_deployment(errors: list[str], label: str, docs: list[dict]) -> dict | None:
    """The one Deployment whose pods are oml-web's (ADR-0070 d4)."""
    found = [d for d in of_kind(docs, "Deployment") if pod_labels(d).get(WEB_COMPONENT[0]) == WEB_COMPONENT[1]]
    if len(found) != 1:
        errors.append(f"{label}: renders {len(found)} oml-web Deployments, not one (ADR-0070 d4)")
        return None
    return found[0]


def lead_service(errors: list[str], label: str, docs: list[dict]) -> dict | None:
    found = [s for s in of_kind(docs, "Service") if s.get("spec", {}).get("selector", {}).get(SERVER_COMPONENT[0]) == SERVER_COMPONENT[1]]
    if len(found) != 1:
        errors.append(f"{label}: renders {len(found)} Services for the server pods, not one")
        return None
    return found[0]


def pod_labels(deployment: dict) -> dict:
    return deployment.get("spec", {}).get("template", {}).get("metadata", {}).get("labels", {})


def env_of(container: dict, name: str) -> dict | None:
    return next((e for e in container.get("env", []) if e.get("name") == name), None)


def check_web(errors: list[str], label: str, docs: list[dict], image_user: str | None) -> None:
    """oml-web beside the lead (ADR-0070 d2, d4)."""
    web = web_deployment(errors, label, docs)
    lead = lead_deployment([], label, docs)
    service = lead_service([], label, docs)
    if web is None or lead is None:
        return
    spec = web.get("spec", {})
    if spec.get("replicas") != 1:
        errors.append(f"{label}: oml-web runs {spec.get('replicas')!r} replicas; the frame route needs one (ADR-0070 d4)")
    labels = pod_labels(web)
    selector = spec.get("selector", {}).get("matchLabels", {})
    if not selector or selector.items() - labels.items():
        errors.append(f"{label}: oml-web's selector {selector!r} does not match its pod template's labels")
    pod = spec.get("template", {}).get("spec", {})
    psc = pod.get("securityContext", {})
    if psc.get("runAsNonRoot") is not True or psc.get("seccompProfile", {}).get("type") != "RuntimeDefault":
        errors.append(f"{label}: oml-web's pod is not non-root under RuntimeDefault seccomp")
    lead_psc = lead.get("spec", {}).get("template", {}).get("spec", {}).get("securityContext", {})
    for field in ("runAsUser", "runAsGroup"):
        if psc.get(field) != lead_psc.get(field):
            errors.append(f"{label}: oml-web's {field} is {psc.get(field)!r}, the lead's {lead_psc.get(field)!r}")
    if pod.get("automountServiceAccountToken") is not False:
        errors.append(f"{label}: oml-web's pod mounts a service account token")
    cs = containers(web)
    if len(cs) != 1:
        errors.append(f"{label}: oml-web's pod runs {len(cs)} containers, not one")
        return
    c = cs[0]
    sc = c.get("securityContext", {})
    if (
        sc.get("readOnlyRootFilesystem") is not True
        or sc.get("allowPrivilegeEscalation") is not False
        or sc.get("capabilities", {}).get("drop") != ["ALL"]
        or sc.get("capabilities", {}).get("add")
    ):
        errors.append(f"{label}: oml-web's container is not read-only, unescalated and without capabilities")
    if c.get("image") != containers(lead)[0].get("image"):
        errors.append(f"{label}: oml-web runs {c.get('image')!r}, not the lead's image {containers(lead)[0].get('image')!r}")
    if c.get("command") != ["/usr/local/bin/oml-web"]:
        errors.append(f"{label}: oml-web's command is {c.get('command')!r}, not the image's /usr/local/bin/oml-web")
    port = {p.get("name"): p.get("containerPort") for p in c.get("ports", [])}.get("http")
    listen = (env_of(c, "OML_WEB__LISTEN") or {}).get("value", "")
    if port is None or not listen.endswith(f":{port}"):
        errors.append(f"{label}: oml-web listens on {listen!r}, its http port is {port!r}")
    for probe, path in WEB_PROBES.items():
        get = c.get(probe, {}).get("httpGet", {})
        if get.get("path") != path or get.get("port") not in ("http", port):
            errors.append(f"{label}: oml-web's {probe} is {c.get(probe)!r}, not GET {path} on the http port")
    api_url = (env_of(c, "OML_WEB__API_URL") or {}).get("value", "")
    if service is not None:
        want = f"http://{service['metadata']['name']}:8080"
        if api_url != want:
            errors.append(f"{label}: oml-web's OML_WEB__API_URL is {api_url!r}, not the lead's Service {want!r}")
    if api_url.startswith("http://") and (env_of(c, "OML_WEB__ALLOW_PLAIN_HTTP_API") or {}).get("value") != "true":
        errors.append(f"{label}: oml-web reaches the lead over plain http without OML_WEB__ALLOW_PLAIN_HTTP_API; it refuses to start")
    if not (env_of(c, "OML_WEB__PUBLIC_URL") or {}).get("value"):
        errors.append(f"{label}: oml-web has no OML_WEB__PUBLIC_URL, and the lead names no issuer it could read")
    refs = []
    for container, who in ((c, "oml-web"), (containers(lead)[0], "the lead")):
        ref = ((env_of(container, "OML_WEB__CLIENT_SECRET") or {}).get("valueFrom") or {}).get("secretKeyRef")
        if ref is None:
            errors.append(f"{label}: {who}'s OML_WEB__CLIENT_SECRET does not come from a secretKeyRef (ADR-0070 d2)")
        refs.append(ref)
    if None not in refs and refs[0] != refs[1]:
        errors.append(f"{label}: the lead and oml-web read OML_WEB__CLIENT_SECRET from {refs[1]!r} and {refs[0]!r}")
    if None not in refs:
        secrets = {d["metadata"]["name"]: d for d in of_kind(docs, "Secret")}
        rendered = secrets.get(refs[0].get("name"))
        if rendered is not None and refs[0].get("key") not in (rendered.get("data") or {}):
            errors.append(f"{label}: Secret {refs[0].get('name')!r} has no key {refs[0].get('key')!r}")
    services = [
        s for s in of_kind(docs, "Service")
        if s.get("spec", {}).get("selector", {}).get(WEB_COMPONENT[0]) == WEB_COMPONENT[1]
    ]
    if len(services) != 1:
        errors.append(f"{label}: renders {len(services)} Services for oml-web, not one")
    else:
        sspec = services[0].get("spec", {})
        if sspec.get("type", "ClusterIP") != "ClusterIP":
            errors.append(f"{label}: oml-web's Service is a {sspec.get('type')}, not cluster-internal")
        if not any(p.get("name") == "http" and p.get("targetPort") in ("http", port) for p in sspec.get("ports", [])):
            errors.append(f"{label}: oml-web's Service has no http port targeting its pods' http port")


def check_web_policy(errors: list[str], label: str, docs: list[dict]) -> None:
    policies = [
        p for p in of_kind(docs, "NetworkPolicy")
        if p.get("spec", {}).get("podSelector", {}).get("matchLabels", {}).get(WEB_COMPONENT[0]) == WEB_COMPONENT[1]
    ]
    if len(policies) != 1:
        errors.append(f"{label}: renders {len(policies)} NetworkPolicy objects for oml-web, not one")
        return
    pspec = policies[0].get("spec", {})
    if sorted(pspec.get("policyTypes", [])) != ["Egress", "Ingress"]:
        errors.append(f"{label}: oml-web's NetworkPolicy does not govern both Ingress and Egress")
    in_ports = [p for rule in pspec.get("ingress", []) for p in rule.get("ports", [])]
    if not in_ports or any(p.get("port") != "http" for p in in_ports):
        errors.append(f"{label}: oml-web's NetworkPolicy admits more than its http port")
    egress = pspec.get("egress", [])
    dns = all(p.get("port") == 53 for p in egress[0].get("ports", [])) if egress else False
    lead = egress[1:] and all(
        (peer.get("podSelector") or {}).get("matchLabels", {}).get(SERVER_COMPONENT[0]) == SERVER_COMPONENT[1]
        for rule in egress[1:]
        for peer in rule.get("to", [{}])
    )
    if len(egress) != 2 or not dns or not lead:
        errors.append(f"{label}: oml-web's NetworkPolicy egress is {egress!r}, not DNS and the lead's pods alone")


def check_notes(errors: list[str]) -> None:
    plain = notes(errors)
    warned = notes(errors, "--set", "ingress.enabled=true")
    if plain is None or warned is None:
        return
    for needle in ("port-forward", "bootstrap claim", "ingress.enabled=true"):
        if needle not in plain:
            errors.append(f"NOTES.txt does not name {needle!r} (ADR-0033 d8)")
    if "WARNING" in plain:
        errors.append("NOTES.txt warns about the Ingress on an install that does not enable it")
    if "WARNING" not in warned:
        errors.append("NOTES.txt does not warn when an install enables the Ingress (ADR-0033 d8)")


def notes(errors: list[str], *args: str) -> str | None:
    """NOTES.txt as a first install renders it.

    `helm install --dry-run` needs a reachable cluster, and `helm template`
    leaves NOTES.txt out, so a copy of the chart renders the same text inside
    a ConfigMap that `helm template` does print.
    """
    with tempfile.TemporaryDirectory() as tmp:
        copy = Path(tmp) / CHART.name
        shutil.copytree(CHART, copy)
        source = copy / "templates/NOTES.txt"
        if not source.is_file():
            errors.append("deploy/helm/oml/templates/NOTES.txt is missing")
            return None
        text = source.read_text()
        source.unlink()
        (copy / "templates/_check_notes.tpl").write_text('{{- define "check.notes" -}}\n' + text + "{{- end -}}\n")
        (copy / "templates/check-notes.yaml").write_text(
            "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: check-notes\n"
            'data:\n  notes: {{ include "check.notes" . | toJson }}\n'
        )
        done = helm(
            "template", RELEASE, str(copy), "--namespace", NAMESPACE,
            "--show-only", "templates/check-notes.yaml", *args,
        )
    if done.returncode != 0:
        errors.append(f"rendering NOTES.txt {' '.join(args)} failed:\n{done.stderr}")
        return None
    return yaml.safe_load(done.stdout)["data"]["notes"]


def helm(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["helm", *args], capture_output=True, text=True, check=False)


class StrictLoader(yaml.SafeLoader):
    """A loader that refuses a mapping key defined twice, as the API server's decoding does."""


def construct_mapping(loader: StrictLoader, node: yaml.MappingNode, deep: bool = False) -> dict:
    keys = [loader.construct_object(k, deep=deep) for k, _ in node.value]
    twice = sorted({str(k) for k in keys if keys.count(k) > 1})
    if twice:
        raise yaml.constructor.ConstructorError(None, None, f"mapping keys defined twice: {twice}", node.start_mark)
    return yaml.SafeLoader.construct_mapping(loader, node, deep=deep)


StrictLoader.add_constructor(yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, construct_mapping)


def render(errors: list[str], *args: str) -> list[dict] | None:
    done = helm("template", RELEASE, str(CHART), "--namespace", NAMESPACE, *args)
    if done.returncode != 0:
        errors.append(f"helm template {' '.join(args)} failed:\n{done.stderr}")
        return None
    try:
        return [d for d in yaml.load_all(done.stdout, Loader=StrictLoader) if d]
    except yaml.YAMLError as e:
        errors.append(f"helm template {' '.join(args)} renders YAML the API server refuses: {e}")
        return None


def of_kind(docs: list[dict], kind: str) -> list[dict]:
    return [d for d in docs if d.get("kind") == kind]


def only(errors: list[str], label: str, docs: list[dict], kind: str) -> dict | None:
    found = of_kind(docs, kind)
    if len(found) != 1:
        errors.append(f"{label}: renders {len(found)} {kind} objects, not one")
        return None
    return found[0]


def containers(deployment: dict) -> list[dict]:
    return deployment.get("spec", {}).get("template", {}).get("spec", {}).get("containers", [])


def report(errors: list[str]) -> int:
    for e in errors:
        print(f"chart check: {e}", file=sys.stderr)
    if not errors:
        print("chart check: deploy/helm/oml renders one hardened lead and oml-web beside it at the workspace version (ADR-0033, ADR-0070)")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
