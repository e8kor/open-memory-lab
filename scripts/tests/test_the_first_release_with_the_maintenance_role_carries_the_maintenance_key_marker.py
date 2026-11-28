"""The first release whose key roles include `maintenance` carries the deployer's maintenance-key
marker in its notes, and no later one does (scripts/release_markers.py; ADR-0097 d18)."""
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import release_markers as M  # noqa: E402

RULESET = '/// The rule-set version.\npub const RULESET: &str = "oml-scan/1.2";\n'


def repo(history):
    """One commit and tag per (tag, maintenance) of `history`, the rule set unchanged."""
    d = tempfile.TemporaryDirectory()
    root = Path(d.name)

    def git(*args):
        subprocess.run(["git", "-C", str(root), "-c", "user.name=t", "-c", "user.email=t@t", *args],
                       check=True, capture_output=True)

    git("init", "-q")
    scanner = root / M.SCANNER_SOURCE
    scanner.parent.mkdir(parents=True)
    scanner.write_text(RULESET)
    roles = root / M.ROLE_SOURCE
    roles.parent.mkdir(parents=True)
    for tag, maintenance in history:
        body = '            Role::Admin => "admin",\n'
        if maintenance:
            body += '            Role::Maintenance => "maintenance",\n'
        roles.write_text(body)
        git("add", "-A")
        git("commit", "-q", "--allow-empty", "-m", tag)
        git("tag", tag)
    return d, root


class MaintenanceMarker(unittest.TestCase):
    def test_the_release_that_adds_the_role_requires_the_marker_and_the_next_does_not(self):
        d, root = repo([("v0.8.5", False), ("v0.8.6", True), ("v0.8.7", True)])
        with d:
            wanted = M.required(root, "v0.8.6")
            self.assertEqual([m for m, _ in wanted], [M.MAINTENANCE_MARKER])
            self.assertIn("new at v0.8.6, absent at v0.8.5", wanted[0][1])
            self.assertEqual(M.required(root, "v0.8.7"), [])

    def test_a_release_before_the_role_requires_no_marker(self):
        d, root = repo([("v0.8.4", False), ("v0.8.5", False)])
        with d:
            self.assertEqual(M.required(root, "v0.8.5"), [])


if __name__ == "__main__":
    unittest.main()
