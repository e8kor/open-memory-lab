"""site_inputs() is the landing's repository's check source (ADR-0070 d12, as amended 2026-11-14):
it returns every file that repository vendors, and this repository writes none of them."""
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import gen_design_tokens as G  # noqa: E402


class SiteInputs(unittest.TestCase):
    def test_site_inputs_returns_the_tokens_the_fonts_and_the_brand_the_landing_vendors(self):
        got = G.site_inputs(G.tokens())
        self.assertEqual(
            set(got),
            {G.SITE_DESIGN / "tokens.css", G.SITE_DESIGN / "fonts.css", G.SITE_DESIGN / "brand.json"},
        )
        brand = json.loads(got[G.SITE_DESIGN / "brand.json"])
        for key in ("_generated", "ground", "favicon", "wordmark", "mark", "gate_wide", "gate_tall", "glyphs"):
            self.assertIn(key, brand)
        self.assertIn(b"@font-face", got[G.SITE_DESIGN / "fonts.css"])
        self.assertEqual(got[G.SITE_DESIGN / "tokens.css"], G.tokens_css(G.tokens()).encode())

    def test_this_repository_writes_none_of_the_site_inputs(self):
        self.assertFalse(set(G.site_inputs(G.tokens())) & set(G.outputs()))
        self.assertFalse(G.SITE_DESIGN.exists())


if __name__ == "__main__":
    unittest.main()
