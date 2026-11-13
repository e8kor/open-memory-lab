"""The web bench counts a page's sequential API rounds and its time in the lead the way its page says."""
import importlib.util
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import gen_bench_pages  # noqa: E402

spec = importlib.util.spec_from_file_location("web_run", ROOT / "bench/web/run.py")
run = importlib.util.module_from_spec(spec)
spec.loader.exec_module(run)


class Rounds(unittest.TestCase):
    def test_two_overlapping_calls_then_one_after_them_are_two_rounds(self):
        self.assertEqual(run.rounds([(0, 10), (2, 9), (11, 20)]), 2)

    def test_calls_that_all_overlap_are_one_round(self):
        self.assertEqual(run.rounds([(0, 10), (1, 12), (5, 6)]), 1)

    def test_a_page_with_no_call_has_no_round(self):
        self.assertEqual(run.rounds([]), 0)

    def test_the_time_in_the_lead_counts_overlapping_calls_once(self):
        self.assertEqual(run.union_ms([(0, 4000), (2000, 6000), (10000, 11000)]), 7.0)


class WebPage(unittest.TestCase):
    def doc(self, shape, p95):
        page = {"page_ms": {"p50": p95 - 1, "p95": p95},
                "api_calls": {"min": 2, "max": 2}, "sequential_rounds": {"min": 1, "max": 1},
                "outside_lead_ms": {"p95": 1.0}}
        return {"shape": shape, "workareas": 6, "date": "2026-11-11", "commit": "c",
                "_path": f"{shape}.json", "seeded": {"memories": 10},
                "healthz_ms": {"p50": 0.4, "p95": 0.5},
                "method": {"iterations": 1, "warmup": 0},
                "environment": {"loadavg_before": [1], "loadavg_after": [1], "cpus": 8,
                                "postgres": "pg"},
                "pages": {"review": page}}

    def test_a_page_more_than_the_budget_above_its_in_process_p95_is_out_of_budget(self):
        over = gen_bench_pages.web_page({6: {"in-process": self.doc("in-process", 6.0),
                                             "oml-web": self.doc("oml-web", 11.5)}})
        self.assertIn("| `review` | 5.0 | 6.0 | 10.5 | 11.5 | +5.5 | no |", over)
        within = gen_bench_pages.web_page({6: {"in-process": self.doc("in-process", 6.0),
                                               "oml-web": self.doc("oml-web", 11.0)}})
        self.assertIn("| +5.0 | yes |", within)

    def test_a_re_baselined_page_is_judged_against_its_re_set_baseline_only_where_it_is_passed(self):
        runs = {6: {"in-process": self.doc("in-process", 6.0),
                    "oml-web": self.doc("oml-web", 21.4)}}
        plain = gen_bench_pages.web_page(runs)
        self.assertIn("| `review` | 5.0 | 6.0 | 20.4 | 21.4 | +15.4 | no |", plain)
        rebased = gen_bench_pages.web_page(runs, {"review": {6: 21.4}})
        self.assertIn("| +15.4 | yes, re-baselined at 21.4 |", rebased)

    def test_the_recorded_runs_review_page_is_re_baselined_in_the_built_page(self):
        built = gen_bench_pages.pages()["web.md"]
        for base in gen_bench_pages.WEB_REBASELINED["review"].values():
            self.assertIn(f"re-baselined at {base}", built)


if __name__ == "__main__":
    unittest.main()
