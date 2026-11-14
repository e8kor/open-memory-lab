import json
import re
import tempfile
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


class ArchitectureViews(unittest.TestCase):
    """The architecture views (docs/architecture/views.md): gen_design_tokens.arch_view."""

    def view(self, body: str, style: str = ".box{fill:var(--ground-alt);stroke:var(--edge)}") -> Path:
        d = Path(tempfile.mkdtemp())
        src = d / "arch-test.svg"
        src.write_text(f"<!-- a test view -->\n<svg><style>{style}</style>{body}</svg>\n")
        return src

    def colour(self, theme: str, name: str) -> str:
        return next(tok["value"][theme] for tok in G.tokens()["color"]["tokens"] if tok["name"] == name)

    def test_an_architecture_view_resolves_to_paper_to_night_and_to_both_under_the_media_query(self):
        t, src = G.tokens(), self.view('<rect class="box"/>')
        paper, night = self.colour("light", "ground-alt"), self.colour("dark", "ground-alt")
        light, dark, both = (G.arch_view(src, t, theme) for theme in ("light", "dark", "adaptive"))
        self.assertIn(paper, light)
        self.assertNotIn(night, light)
        self.assertNotIn("prefers-color-scheme", light)
        self.assertIn(night, dark)
        self.assertNotIn(paper, dark)
        self.assertNotIn("prefers-color-scheme", dark)
        before, after = both.split("@media (prefers-color-scheme: dark)")
        self.assertIn(paper, before)
        self.assertIn(night, after)
        for out in (light, dark, both):
            self.assertNotIn("var(--", out)

    def test_every_architecture_source_in_design_diagrams_is_a_generated_view(self):
        sources = sorted(p.stem.removeprefix("arch-") for p in (G.DESIGN / "diagrams").glob("arch-*.svg"))
        self.assertEqual(sources, sorted(G.ARCH_VIEWS))

    def test_a_view_that_draws_a_designed_element_says_so_in_its_text_alternative(self):
        for view in G.ARCH_VIEWS:
            text = (G.DESIGN / "diagrams" / f"arch-{view}.svg").read_text()
            if ">designed</text>" in text:
                desc = text.split("<desc", 1)[1].split("</desc>", 1)[0]
                self.assertIn("designed", desc, view)

    def test_every_box_or_region_carrying_a_designed_tag_is_drawn_dashed(self):
        # A tag sits on the top edge of the shape it marks, 74 px left of its right edge and 8 px
        # above it (design/diagrams/arch-*.svg); a tag beside an arrow or in a legend marks no shape.
        shape = re.compile(r'<rect class="([^"]*)" x="([\d.]+)" y="([\d.]+)" width="([\d.]+)" height="[\d.]+"')
        tag = re.compile(r'<rect class="tag" x="([\d.]+)" y="([\d.]+)"')
        checked = 0
        for view in G.ARCH_VIEWS:
            text = (G.DESIGN / "diagrams" / f"arch-{view}.svg").read_text()
            shapes = [(c, float(x), float(y), float(w)) for c, x, y, w in shape.findall(text)]
            for tx, ty in tag.findall(text):
                for cls, x, y, w in shapes:
                    if cls != "tag" and y == float(ty) + 8 and x + w - 74 == float(tx):
                        checked += 1
                        self.assertIn("plan", cls.split(), f"{view}: a designed tag on a solid {cls} at x={x}")
        self.assertGreaterEqual(checked, 20)

    def test_an_architecture_view_with_a_colour_outside_its_style_is_refused(self):
        src = self.view('<rect style="fill:var(--ground)"/>')
        with self.assertRaisesRegex(ValueError, "outside the style element"):
            G.arch_view(src, G.tokens(), "light")

    def test_an_architecture_view_naming_an_unknown_token_is_refused_by_the_tokens_name(self):
        src = self.view("<rect/>", style=".box{fill:var(--no-such-colour)}")
        with self.assertRaisesRegex(ValueError, "--no-such-colour"):
            G.arch_view(src, G.tokens(), "dark")


if __name__ == "__main__":
    unittest.main()
