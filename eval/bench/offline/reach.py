#!/usr/bin/env python3
"""How far a sparse model's tokenizer and idf table reach beyond English (ADR-0052 Q6).

    reach.py <model>

For one short sentence per language, the share of word pieces that are
[UNK], the pieces per whitespace word, and the share of the query's pieces
that carry a non-zero idf weight (a piece without one contributes nothing
to a sparse score). No qrels exist for these languages here, so this bounds
what the query side can match; it does not measure quality.
"""

import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import evaluate  # noqa: E402

SENTENCES = {
    "English": ("The invoice service retries a failed payment three times before it alerts "
                "the team."),
    "German": ("Der Rechnungsdienst wiederholt eine fehlgeschlagene Zahlung dreimal, bevor er "
               "das Team warnt."),
    "Polish": "Usługa faktur ponawia nieudaną płatność trzy razy, zanim powiadomi zespół.",
    "Ukrainian": "Служба рахунків повторює невдалий платіж тричі, перш ніж сповістити команду.",
    "Chinese": "发票服务在通知团队之前会重试失败的付款三次。",
    "Japanese": "請求サービスは、チームに通知する前に失敗した支払いを3回再試行します。",
    "Arabic": "تعيد خدمة الفواتير محاولة الدفع الفاشل ثلاث مرات قبل تنبيه الفريق.",
}


def main():
    from transformers import AutoTokenizer

    model = sys.argv[1]
    tok = AutoTokenizer.from_pretrained(evaluate.SPARSE[model])
    enc = evaluate.sparse_query_encoder(model)
    print("| language | pieces per word | [UNK] share | pieces with idf > 0 |")
    print("|---|---:|---:|---:|")
    for lang, s in SENTENCES.items():
        pieces = tok.tokenize(s)
        words = max(1, len(s.split()))
        unk = sum(1 for p in pieces if p == tok.unk_token) / max(1, len(pieces))
        ids, w = enc(s)
        live = float(np.mean(w > 0)) if len(w) else 0.0
        print(f"| {lang} | {len(pieces) / words:.1f} | {unk:.2f} | {live:.2f} |")


if __name__ == "__main__":
    main()
