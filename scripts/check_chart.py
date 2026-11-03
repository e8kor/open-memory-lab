"""The Helm chart renders the one lead ADR-0033 decides, at the workspace version.

Run by .github/workflows/ci.yml (job chart). Renders deploy/helm/oml with
`helm template` and checks, failing with every disagreement found:

- Chart.yaml's version and appVersion are the workspace version (Cargo.toml),
  so one tag names the binaries, the image, the plugin and the chart (d9);
- `helm lint --strict` passes with the default values and with each
  ci/*-values.yaml file;
- the default image is ghcr.io/e8kor/open-memory-lab:<appVersion>, and
  image.digest pins it as <repository>:<tag>@<digest> (d9);
- exactly one Deployment, at one replica, with the Recreate strategy (d2);
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
SERVER_COMPONENT = ("app.kubernetes.io/component", "server")
LEAD_ROLE = ("oml.e8kor.github.io/role", "lead")
PROBES = {"startupProbe": "/healthz", "livenessProbe": "/healthz", "readinessProbe": "/readyz"}


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
        image = only(errors, label, docs, "Deployment")
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
        lead = only(errors, "default values", defaults, "Deployment")
        if lead is not None:
            for c in containers(lead):
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
    deployment = only(errors, label, docs, "Deployment")
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

    service = only(errors, label, docs, "Service")
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
    deployment = only(errors, label, docs, "Deployment")
    service = only(errors, label, docs, "Service")
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

    policy = only(errors, label, docs, "NetworkPolicy")
    if policy is None:
        return
    pspec = policy.get("spec", {})
    selected = pspec.get("podSelector", {}).get("matchLabels", {})
    if dict([SERVER_COMPONENT]).items() - selected.items():
        errors.append(f"{label}: the NetworkPolicy does not select the server pods: {selected!r}")
    if sorted(pspec.get("policyTypes", [])) != ["Egress", "Ingress"]:
        errors.append(f"{label}: the NetworkPolicy does not govern both Ingress and Egress")
    in_ports = [p for rule in pspec.get("ingress", []) for p in rule.get("ports", [])]
    if not pspec.get("ingress") or any(not r.get("ports") for r in pspec["ingress"]) or any(
        p.get("port") != "http" for p in in_ports
    ):
        errors.append(f"{label}: the NetworkPolicy admits more than the API port: {pspec.get('ingress')!r}")
    out_ports = sorted({p.get("port") for rule in pspec.get("egress", []) for p in rule.get("ports", [])}, key=str)
    if not pspec.get("egress") or any(not r.get("ports") for r in pspec["egress"]) or out_ports != sorted({53, 5432}, key=str):
        errors.append(f"{label}: the NetworkPolicy's egress is {pspec.get('egress')!r}, not DNS and the Postgres port alone")


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
        print("chart check: deploy/helm/oml renders one hardened lead at the workspace version (ADR-0033)")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
