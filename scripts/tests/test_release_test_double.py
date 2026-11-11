"""No release turns on CI's sign-in double (scripts/check_release.py, check_no_test_double;
ADR-0070, phase (b) as built): neither by the feature's name nor through the image's
OML_CARGO_FEATURES build argument, whose value a workflow variable could carry (#242)."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import check_release as R  # noqa: E402

DOCKERFILE = 'ARG OML_CARGO_FEATURES=""\n'


class TestDouble(unittest.TestCase):
    def test_the_release_workflow_and_the_dockerfile_as_committed_pass(self):
        errors: list[str] = []
        R.check_no_test_double(errors, R.WORKFLOW.read_text(), (R.ROOT / "Dockerfile").read_text())
        self.assertEqual(errors, [])

    def test_a_release_workflow_that_names_the_feature_is_refused(self):
        errors: list[str] = []
        R.check_no_test_double(errors, "features: oml-server-lead/e2e-sign-in\n", DOCKERFILE)
        self.assertEqual(len(errors), 1)
        self.assertIn("e2e-sign-in", errors[0])

    def test_a_release_workflow_that_sets_the_image_features_is_refused_whatever_their_value(self):
        workflow = "build-args: |\n  OML_CARGO_FEATURES=${{ vars.FEATURES }}\n"
        errors: list[str] = []
        R.check_no_test_double(errors, workflow, DOCKERFILE)
        self.assertEqual(len(errors), 1)
        self.assertIn("OML_CARGO_FEATURES", errors[0])

    def test_a_dockerfile_whose_features_default_to_anything_is_refused(self):
        for dockerfile in ("FROM scratch\n", 'ARG OML_CARGO_FEATURES="oml-server-lead/e2e-sign-in"\n'):
            errors: list[str] = []
            R.check_no_test_double(errors, "", dockerfile)
            self.assertEqual(len(errors), 1, dockerfile)


if __name__ == "__main__":
    unittest.main()
