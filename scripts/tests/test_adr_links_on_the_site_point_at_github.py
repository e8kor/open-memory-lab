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

    def test_a_link_to_a_former_record_name_fails_the_strict_build_naming_the_current_one(self):
        with self.assertLogs("mkdocs.plugins.adr_links", "WARNING") as logs:
            H.rewrite("[a](../adr/ADR-0008-workarea-algebra.md)", "architecture/overview.md")
        self.assertIn("ADR-0008-workarea-algebra.md was renamed to ADR-0008-workspace-algebra.md", logs.output[0])

    def test_a_link_to_a_record_that_does_not_exist_fails_the_strict_build(self):
        with self.assertLogs("mkdocs.plugins.adr_links", "WARNING") as logs:
            H.rewrite("[a](adr/ADR-0999-no-such-record.md)", "faq.md")
        self.assertIn("ADR-0999-no-such-record.md does not exist", logs.output[0])

    def test_every_renamed_record_row_names_a_former_file_that_is_gone_and_a_current_one_that_exists(self):
        rows = H.renamed()
        self.assertGreaterEqual(len(rows), 12)
        for former, current in rows.items():
            self.assertFalse((H.ADR_DIR / former).exists(), former)
            self.assertTrue((H.ADR_DIR / current).is_file(), current)
            self.assertEqual(former[:8], current[:8], (former, current))


if __name__ == "__main__":
    unittest.main()
