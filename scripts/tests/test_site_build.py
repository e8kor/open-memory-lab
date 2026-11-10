import shutil
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "site"))
sys.path.insert(0, str(ROOT / "scripts"))
import build as S  # noqa: E402
import gen_design_tokens as G  # noqa: E402


class SiteBuild(unittest.TestCase):
    def test_site_dist_matches_its_sources(self):
        self.assertEqual(0, S.main(["--check"]))

    def test_the_design_inputs_of_the_site_match_the_design_system(self):
        for path, data in G.site_inputs(G.tokens()).items():
            self.assertEqual(data, path.read_bytes(), path)

    def test_an_unfilled_placeholder_fails_the_build(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / "src"
            shutil.copytree(S.SRC, src)
            page = src / "teams.src.html"
            page.write_text(page.read_text().replace("</main>", "{{NOT_A_PLACEHOLDER}}</main>", 1))
            with self.assertRaisesRegex(ValueError, "unfilled placeholders"):
                S.pages(src=src)

    def test_a_glyph_the_design_system_lacks_fails_the_build(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / "src"
            shutil.copytree(S.SRC, src)
            page = src / "index.src.html"
            page.write_text(page.read_text().replace("</main>", "{{GLYPH:no-such-glyph}}</main>", 1))
            with self.assertRaisesRegex(ValueError, "no-such-glyph"):
                S.pages(src=src)

    def test_the_footer_links_the_legal_pages_only_when_asked(self):
        plain = S.pages()["index.html"].decode()
        legal = S.pages(legal=True)["index.html"].decode()
        self.assertNotIn('href="/privacy"', plain)
        self.assertIn('href="/privacy"', legal)

    def test_the_footer_names_the_workspace_version(self):
        self.assertIn(f"memory.doublegate.io v{S.workspace_version()}", S.pages()["index.html"].decode())

    def test_a_stray_file_in_dist_fails_the_check(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            self.assertEqual(0, S.main(["--out", str(out)]))
            self.assertEqual(0, S.main(["--check", "--out", str(out)]))
            (out / "stray.html").write_text("x")
            self.assertEqual(1, S.main(["--check", "--out", str(out)]))


if __name__ == "__main__":
    unittest.main()
