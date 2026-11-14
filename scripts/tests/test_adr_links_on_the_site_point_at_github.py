import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "hooks"))
import adr_links as H  # noqa: E402

BLOB = "https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/"


class AdrLinks(unittest.TestCase):
    def test_a_relative_link_to_a_record_points_at_its_blob_with_its_fragment(self):
        md = "See [roles](../adr/ADR-0010-roles-and-keys.md#decision) and [c](../adr/ADR-0001-the-constitution.md)."
        out = H.rewrite(md, "architecture/overview.md")
        self.assertIn(f"]({BLOB}ADR-0010-roles-and-keys.md#decision)", out)
        self.assertIn(f"]({BLOB}ADR-0001-the-constitution.md)", out)

    def test_a_link_from_the_docs_root_and_a_reference_link_are_rewritten(self):
        self.assertIn(f"]({BLOB}ADR-0001-the-constitution.md)", H.rewrite("[R1](adr/ADR-0001-the-constitution.md)", "faq.md"))
        out = H.rewrite("[r]: ../adr/ADR-0005-erasure-by-crypto-shredding.md\n", "product/compliance.md")
        self.assertEqual(f"[r]: {BLOB}ADR-0005-erasure-by-crypto-shredding.md\n", out)

    def test_every_other_link_is_left_as_written(self):
        md = ("[a](roles-and-keys.md) [b](../adr/index.md) [c](https://example.com/adr/ADR-0001-x.md) "
              "[d](#the-admin-read) [e](../data/adr/ADR-0001-x.md)")
        self.assertEqual(md, H.rewrite(md, "architecture/overview.md"))

    def test_the_blob_is_the_design_records_page_s(self):
        self.assertEqual(BLOB, H.BLOB)

    def test_no_published_page_keeps_a_relative_record_link(self):
        import re
        excluded = ("includes/", "contributing/", "adr/", "legal/")
        left = []
        for f in sorted((ROOT / "docs").rglob("*.md")):
            rel = f.relative_to(ROOT / "docs").as_posix()
            if rel.startswith(excluded):
                continue
            out = H.rewrite(f.read_text(), rel)
            left += [f"{rel}: {m}" for m in re.findall(r"\]\(((?:\.\./)*adr/ADR-[^)]*)\)", out)]
        self.assertEqual([], left)


if __name__ == "__main__":
    unittest.main()
