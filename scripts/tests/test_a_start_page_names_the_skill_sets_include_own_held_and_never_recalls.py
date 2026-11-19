"""check_contract_sync.py retires v0.3.0's recall flag include_own_held on a start page (ADR-0039
d13), while the skill set's flag of the same name, which ADR-0080 d11 brought back on
GET /v1/skills, may be named there."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import check_contract_sync as C  # noqa: E402


class RetiredRecallFlag(unittest.TestCase):
    def test_a_recall_line_naming_include_own_held_is_retired(self):
        self.assertEqual(C.retired_names("recall(query, include_own_held=True)"), ["include_own_held"])

    def test_a_skill_set_line_naming_include_own_held_passes(self):
        for line in (
            "| `skills(workspace, include_own_held=, cursor=)` |",
            "`iterSkills({ workspace, include_own_held })`",
            "GET /v1/skills?include_own_held=true",
        ):
            self.assertEqual(C.retired_names(line), [], line)

    def test_the_probes_still_hold(self):
        self.assertEqual(C.retired_name_pattern_problems(), [])


if __name__ == "__main__":
    unittest.main()
