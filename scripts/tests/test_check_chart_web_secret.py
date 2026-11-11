"""The chart guards on the web client secret refuse each break they name (scripts/check_chart.py,
check_web_secret_rotation and check_web_secret_values; ADR-0070 d2, #230, #242, #262): a literal
value, oml-web reading the rotation settings, the lead missing one or reading it from another
Secret or key or as required, a value in values.yaml, and a Secret rendered beside existingSecret.
No test renders the chart: each feeds the guard the objects it reads."""
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import check_chart as C  # noqa: E402

SECRET = "oml-web-client"


def ref(key: str, name: str = SECRET, optional: bool | None = None) -> dict:
    secret_ref = {"name": name, "key": key}
    if optional is not None:
        secret_ref["optional"] = optional
    return {"secretKeyRef": secret_ref}


def env(name: str, value_from: dict) -> dict:
    return {"name": name, "valueFrom": value_from}


def web() -> dict:
    return {"env": [env("OML_WEB__CLIENT_SECRET", ref("OML_WEB__CLIENT_SECRET"))]}


def lead() -> dict:
    return {
        "env": [
            env("OML_WEB__CLIENT_SECRET", ref("OML_WEB__CLIENT_SECRET")),
            env("OML_WEB__CLIENT_SECRET_PREVIOUS", ref("OML_WEB__CLIENT_SECRET_PREVIOUS", optional=True)),
            env(
                "OML_WEB__CLIENT_SECRET_PREVIOUS_UNTIL",
                ref("OML_WEB__CLIENT_SECRET_PREVIOUS_UNTIL", optional=True),
            ),
        ]
    }


def rotation_errors(web_container: dict, lead_container: dict) -> list[str]:
    errors: list[str] = []
    current = {"name": SECRET, "key": "OML_WEB__CLIENT_SECRET"}
    C.check_web_secret_rotation(errors, "default", web_container, lead_container, current)
    return errors


def deployment(name: str, container: dict) -> dict:
    return {
        "kind": "Deployment",
        "metadata": {"name": name},
        "spec": {"template": {"spec": {"containers": [container]}}},
    }


class Rotation(unittest.TestCase):
    def test_the_rotation_as_the_chart_renders_it_passes(self):
        self.assertEqual(rotation_errors(web(), lead()), [])

    def test_a_literal_secret_value_is_refused_on_either_pod(self):
        for who, w, l in (("oml-web", web(), lead()), ("the lead", web(), lead())):
            container = w if who == "oml-web" else l
            container["env"][0] = {"name": "OML_WEB__CLIENT_SECRET", "value": "not-a-ref"}
            errors = rotation_errors(w, l)
            self.assertTrue(any("literal value" in e and who in e for e in errors), errors)

    def test_oml_web_reading_a_rotation_setting_is_refused(self):
        w = web()
        w["env"].append(env("OML_WEB__CLIENT_SECRET_PREVIOUS", ref("OML_WEB__CLIENT_SECRET_PREVIOUS", optional=True)))
        errors = rotation_errors(w, lead())
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("oml-web reads OML_WEB__CLIENT_SECRET_PREVIOUS", errors[0])

    def test_a_lead_missing_a_rotation_setting_is_refused(self):
        for missing in C.WEB_SECRET_ROTATION_ENV:
            l = lead()
            l["env"] = [e for e in l["env"] if e["name"] != missing]
            errors = rotation_errors(web(), l)
            self.assertEqual(len(errors), 1, errors)
            self.assertIn(f"does not read {missing}", errors[0])

    def test_a_rotation_setting_from_another_secret_or_key_or_required_is_refused(self):
        name = "OML_WEB__CLIENT_SECRET_PREVIOUS"
        for broken, words in (
            (ref(name, name="elsewhere", optional=True), "from Secret 'elsewhere'"),
            (ref("OTHER_KEY", optional=True), "from key 'OTHER_KEY'"),
            (ref(name), "is not optional"),
            (ref(name, optional=False), "is not optional"),
        ):
            l = lead()
            l["env"][1] = env(name, broken)
            errors = rotation_errors(web(), l)
            self.assertEqual(len(errors), 1, (broken, errors))
            self.assertIn(words, errors[0])


class Values(unittest.TestCase):
    def values_errors(self, values: str, rendered: list[dict]) -> list[str]:
        with tempfile.TemporaryDirectory() as chart:
            (Path(chart) / "values.yaml").write_text(values)
            errors: list[str] = []
            with mock.patch.object(C, "CHART", Path(chart)), mock.patch.object(C, "render", return_value=rendered):
                C.check_web_secret_values(errors)
            return errors

    def named(self, secret: str = "operator-web-client") -> list[dict]:
        container = {"env": [env("OML_WEB__CLIENT_SECRET", ref("OML_WEB__CLIENT_SECRET", name=secret))]}
        lead_deployment = deployment("oml", container)
        lead_deployment["spec"]["template"]["metadata"] = {"labels": dict([C.SERVER_COMPONENT, C.LEAD_ROLE])}
        web_deployment = deployment("oml-web", container)
        web_deployment["spec"]["template"]["metadata"] = {"labels": dict([C.WEB_COMPONENT])}
        return [lead_deployment, web_deployment]

    VALUES = (
        "web:\n  clientSecret:\n    existingSecret: \"\"\n    key: OML_WEB__CLIENT_SECRET\n"
        "    previousKey: OML_WEB__CLIENT_SECRET_PREVIOUS\n"
        "    previousUntilKey: OML_WEB__CLIENT_SECRET_PREVIOUS_UNTIL\n"
    )

    def test_values_that_name_a_secret_and_a_render_that_reads_it_pass(self):
        self.assertEqual(self.values_errors(self.VALUES, self.named()), [])

    def test_a_secret_value_in_values_yaml_is_refused(self):
        errors = self.values_errors(self.VALUES + "    value: hunter2-hunter2\n", self.named())
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("['value']", errors[0])
        self.assertNotIn("hunter2", errors[0])

    def test_a_secret_rendered_beside_existing_secret_is_refused(self):
        rendered = self.named() + [{"kind": "Secret", "metadata": {"name": "oml-web-client"}}]
        errors = self.values_errors(self.VALUES, rendered)
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("still renders a Secret", errors[0])

    def test_a_pod_that_ignores_existing_secret_is_refused(self):
        errors = self.values_errors(self.VALUES, self.named(secret="oml-web-client"))
        self.assertEqual(len(errors), 2, errors)
        self.assertTrue(all("not the named Secret" in e for e in errors), errors)


if __name__ == "__main__":
    unittest.main()
