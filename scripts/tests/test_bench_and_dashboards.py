"""The scheduled benchmark asks only for pipelines its lead serves, and a dashboard uses what its page says."""
import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class BenchWorkflow(unittest.TestCase):
    def test_the_scheduled_benchmark_asks_for_exactly_the_pipelines_the_booted_lead_serves(self):
        boot = (ROOT / "eval/bench/boot.sh").read_text()
        served = re.search(r'PIPELINES="\$\{BENCH_PIPELINES:-([^}]+)\}"', boot).group(1).split()
        workflow = (ROOT / ".github/workflows/bench.yml").read_text()
        asked = re.search(r'^\s*BENCH_PIPELINES:\s*"?([^"\n]+?)"?\s*$', workflow, re.M)
        self.assertIsNotNone(asked, "bench.yml sets no BENCH_PIPELINES, so bench.py asks for every pipeline it knows, including ones the lead is not booted with")
        self.assertEqual(asked.group(1).split(), served)


class WebTrafficDashboard(unittest.TestCase):
    def test_every_metric_the_page_says_the_dashboard_uses_is_queried_by_a_panel(self):
        page = (ROOT / "docs/operations/observability.md").read_text()
        section = page.split("## First-party web traffic", 1)[1]
        said = set(re.findall(r"`(oml_[a-z_]+)`", section))
        self.assertTrue(said)
        dash = json.loads((ROOT / "docs/operations/dashboards/oml-web-traffic.json").read_text())
        exprs = " ".join(t["expr"] for p in dash["panels"] for t in p.get("targets", []))
        for metric in said:
            self.assertIn(metric, exprs, f"{metric} is named by observability.md and no panel queries it")


if __name__ == "__main__":
    unittest.main()
