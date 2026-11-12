"""check_contract_sync.py holds the Rust SDK's operation table and response structs to
openapi.yaml at every depth (ADR-0070 d9): a mismatched shape fails, not only a wrong path."""
import copy
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import yaml  # noqa: E402

import check_contract_sync as C  # noqa: E402

SPEC = yaml.safe_load(C.OPENAPI_YAML.read_text(encoding="utf-8"))
SOURCE = C.RUST_SDK_RS.read_text(encoding="utf-8")


def renamed(struct: str, field: str, to: str) -> str:
    """SOURCE with `pub <field>:` renamed to `to` inside `pub struct <struct>` only."""
    start = SOURCE.index(f"pub struct {struct} {{")
    end = SOURCE.index("\n}", start)
    body = SOURCE[start:end]
    assert f"pub {field}:" in body, (struct, field)
    return SOURCE[:start] + body.replace(f"pub {field}:", f"pub {to}:") + SOURCE[end:]


class RustSdk(unittest.TestCase):
    def test_the_sdk_as_written_agrees_with_openapi_yaml(self):
        self.assertEqual(C.rust_sdk_problems(SPEC), [])

    def test_a_top_level_response_field_openapi_yaml_lacks_fails(self):
        problems = C.rust_sdk_problems(SPEC, renamed("Whoami", "role", "roles"))
        self.assertEqual(problems, ["whoami: the SDK's Whoami.roles is a field openapi.yaml does not declare"])

    def test_a_nested_field_through_a_list_fails(self):
        problems = C.rust_sdk_problems(SPEC, renamed("ServedGuardrail", "severity", "level"))
        self.assertEqual(
            problems,
            ["getGuardrails.guardrails: the SDK's ServedGuardrail.level is a field openapi.yaml does not declare"],
        )

    def test_a_nested_field_through_an_optional_reference_fails(self):
        problems = C.rust_sdk_problems(SPEC, renamed("NamedRef", "name", "label"))
        self.assertIn("whoami.tenant: the SDK's NamedRef.label is a field openapi.yaml does not declare", problems)
        self.assertIn("whoami.user: the SDK's NamedRef.label is a field openapi.yaml does not declare", problems)

    def test_a_nested_field_through_a_oneof_listing_fails(self):
        problems = C.rust_sdk_problems(SPEC, renamed("WorkareaNode", "path", "route"))
        self.assertEqual(
            problems,
            ["getWorkareaTree.items: the SDK's WorkareaNode.route is a field openapi.yaml does not declare"],
        )

    def test_a_schema_that_declares_no_fields_is_reported_not_skipped(self):
        spec = copy.deepcopy(SPEC)
        spec["paths"]["/v2/whoami"]["get"]["responses"]["200"]["content"]["application/json"]["schema"] = {
            "type": "object"
        }
        problems = C.rust_sdk_problems(spec)
        self.assertEqual(
            problems,
            # Every entry that reads whoami's answer reports it: the one with
            # person=false, the one with start=true and the one with no query,
            # which reads the person block.
            [
                "whoami: the schema Whoami reads declares no fields, so its shape cannot be checked",
                "whoami: the schema Onboarding reads declares no fields, so its shape cannot be checked",
                "whoami: the schema Onboarding reads declares no fields, so its shape cannot be checked",
            ],
        )

    def test_a_resp_type_that_is_no_struct_fails(self):
        source = SOURCE.replace("resp:Whoami", "resp:Whoever")
        self.assertEqual(
            C.rust_sdk_problems(SPEC, source),
            ["whoami: resp:Whoever is no pub struct in sdks/rust/src/lib.rs"],
        )


if __name__ == "__main__":
    unittest.main()
