#!/usr/bin/env python3
"""Print the result tables REPORT.md quotes from <results>/*.json. Standard library only.

    python3 eval/recall/tables.py eval/recall/results > eval/recall/results/tables.md
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import metrics  # noqa: E402

ORDER = ["oml", "oml_keywords", "pg_simple_and", "pg_simple_or", "pg_simple_and_or",
         "pg_english_or", "pg_english_or_trgm", "bm25", "dense", "hybrid_rrf",
         # The served search pipelines through oml_read (ADR-0049), and the
         # failure hook's shape on two of them.
         "served_keyword_all", "served_keyword_any", "served_bm25", "served_bm25_trigram",
         "hook_keyword_all", "hook_bm25_trigram"]


def main():
    results = Path(sys.argv[1])
    found = {p.stem: json.loads(p.read_text()) for p in results.glob("*.json") if p.stem != "idmap"}
    summaries = {m: found[m]["summary"] for m in ORDER if m in found}
    print("## Overall\n")
    print(metrics.overall_table(summaries))
    for metric, title in [("s1", "success@1"), ("s5", "success@5"), ("ndcg10", "nDCG@10"),
                          ("zero", "zero-result rate")]:
        print(f"\n## {title} by category\n")
        print(metrics.markdown_table(summaries, metric))


if __name__ == "__main__":
    main()
