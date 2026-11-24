"""`import oml_sdk` imports neither LangChain nor LangGraph; only
`import oml_sdk.langchain` does, from the `langchain` extra (ADR-0094 d1)."""

import subprocess
import sys

PROBE = """
import sys
import oml_sdk
loaded = sorted(m for m in sys.modules if m.split(".")[0] in {"langchain_core", "langgraph", "langmem"})
print(",".join(loaded))
"""


def test_importing_oml_sdk_imports_no_langchain_package():
    done = subprocess.run([sys.executable, "-c", PROBE], capture_output=True, text=True, check=True)
    assert done.stdout.strip() == ""


def test_importing_oml_sdk_langchain_imports_both_packages_it_needs():
    done = subprocess.run(
        [sys.executable, "-c", PROBE.replace("import oml_sdk\n", "import oml_sdk.langchain\n")],
        capture_output=True,
        text=True,
        check=True,
    )
    assert {"langchain_core", "langgraph"} <= {m.split(".")[0] for m in done.stdout.split(",")}
