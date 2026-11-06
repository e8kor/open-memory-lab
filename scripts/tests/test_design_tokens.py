import json
import unittest
from pathlib import Path

import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import check_contrast as C  # noqa: E402
import gen_design_tokens as G  # noqa: E402


class DesignTokens(unittest.TestCase):
    def test_every_colour_token_reaches_the_web_stylesheet_in_both_themes(self):
        t = G.tokens()
        css = G.tokens_css(t)
        dark, light = css.split("@media (prefers-color-scheme:light)")
        for tok in t["color"]["tokens"]:
            self.assertIn(f"--{tok['name']}:{tok['value']['dark']}", dark)
            self.assertIn(f"--{tok['name']}:{tok['value']['light']}", light)

    def test_a_dotted_token_name_is_escaped(self):
        self.assertIn("--space-0\\.5:2px", G.tokens_css(G.tokens()))

    def test_the_generated_files_match_their_sources(self):
        self.assertEqual(0, G.main(["--check"]))

    def test_a_text_colour_below_its_floor_fails_the_contrast_check(self):
        t = json.loads((G.DESIGN / "tokens.json").read_text())
        for tok in t["color"]["tokens"]:
            if tok["name"] == "ink-faint":
                tok["value"]["dark"] = "#2a2d44"
        fails = [r for r in C.pairs(t) if r[5] < r[6]]
        self.assertTrue(any(r[1] == "ink-faint" and r[0] == "dark" for r in fails))
        self.assertEqual([], [r for r in C.pairs(G.tokens()) if r[5] < r[6]])


if __name__ == "__main__":
    unittest.main()
