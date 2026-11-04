# EXPLAIN (ANALYZE, BUFFERS) excerpts

Universe-scope queries (every workarea readable), as the serving role with oml.tenant_id set, and the same statement as the superuser (`@no_rls`). Long literals are elided.


## 10k


### a_simple_and — exact query

```
Limit (actual time=121.693..121.696 rows=5.00 loops=1)
  Buffers: shared hit=3859
  ->  Sort (actual time=121.691..121.693 rows=5.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''canary'' & ''5'' & ''traffic'''::tsquery, 0)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=3859
        ->  Result (actual time=34.005..121.629 rows=5.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              Buffers: shared hit=3853
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=33.962..121.419 rows=5.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''canary'' & ''5'' & ''traffic'''::tsquery))
                    Rows Removed by Filter: 6517
                    Buffers: shared hit=3853
Planning Time: 2.759 ms
Execution Time: 121.784 ms
```

### a_simple_and — long query

```
Limit (actual time=105.420..105.422 rows=0.00 loops=1)
  Buffers: shared hit=3859
  ->  Sort (actual time=105.418..105.420 rows=0.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery, 0)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=3859
        ->  Result (actual time=105.348..105.349 rows=0.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              Buffers: shared hit=3853
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=105.342..105.342 rows=0.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery))
                    Rows Removed by Filter: 6522
                    Buffers: shared hit=3853
Planning Time: 2.615 ms
Execution Time: 105.490 ms
```

### a_simple_and@no_rls — exact query

```
Limit (actual time=0.575..0.578 rows=5.00 loops=1)
  Buffers: shared hit=19
  ->  Sort (actual time=0.574..0.575 rows=5.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''canary'' & ''5'' & ''traffic'''::tsquery, 0)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=19
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=0.239..0.522 rows=5.00 loops=1)
              Recheck Cond: (to_tsvector('simple'::regconfig, claim) @@ '''canary'' & ''5'' & ''traffic'''::tsquery)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 1
              Heap Blocks: exact=6
              Buffers: shared hit=13
              ->  Bitmap Index Scan on memories_t_<tenant>_claim_fts (actual time=0.092..0.092 rows=6.00 loops=1)
                    Index Cond: (to_tsvector('simple'::regconfig, claim) @@ '''canary'' & ''5'' & ''traffic'''::tsquery)
                    Buffers: shared hit=7
Planning Time: 3.039 ms
Execution Time: 0.697 ms
```

### a_simple_and@no_rls — long query

```
Limit (actual time=0.476..0.476 rows=0.00 loops=1)
  ->  Sort (actual time=0.475..0.475 rows=0.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery, 0)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=0.431..0.432 rows=0.00 loops=1)
              Recheck Cond: (to_tsvector('simple'::regconfig, claim) @@ '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              ->  Bitmap Index Scan on memories_t_<tenant>_claim_fts (actual time=0.426..0.426 rows=0.00 loops=1)
                    Index Cond: (to_tsvector('simple'::regconfig, claim) @@ '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery)
Planning Time: 1.852 ms
Execution Time: 0.520 ms
```

### b_simple_or_rank — exact query

```
Limit (actual time=109.272..109.275 rows=10.00 loops=1)
  Buffers: shared hit=3859
  ->  Sort (actual time=109.270..109.271 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''canary'' | ''5'' | ''traffic'''::tsquery, 0)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=3859
        ->  Result (actual time=0.543..109.127 rows=236.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              Buffers: shared hit=3853
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.529..104.453 rows=236.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''canary'' | ''5'' | ''traffic'''::tsquery))
                    Rows Removed by Filter: 6286
                    Buffers: shared hit=3853
Planning Time: 1.273 ms
Execution Time: 109.320 ms
```

### b_simple_or_rank — long query

```
Limit (actual time=188.678..188.680 rows=10.00 loops=1)
  Buffers: shared hit=3859
  ->  Sort (actual time=188.676..188.678 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=3859
        ->  Result (actual time=0.126..187.798 rows=5903.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              Buffers: shared hit=3853
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.082..96.516 rows=5903.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery))
                    Rows Removed by Filter: 619
                    Buffers: shared hit=3853
Planning Time: 2.137 ms
Execution Time: 188.733 ms
```

### b_simple_or_rank@no_rls — exact query

```
Limit (actual time=14.635..14.639 rows=10.00 loops=1)
  Buffers: shared hit=272
  ->  Sort (actual time=14.633..14.635 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''canary'' | ''5'' | ''traffic'''::tsquery, 0)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=272
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=1.328..14.490 rows=236.00 loops=1)
              Recheck Cond: ((to_tsvector('simple'::regconfig, claim) @@ '''canary'' | ''5'' | ''traffic'''::tsquery) AND (tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
              Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])))
              Rows Removed by Filter: 19
              Heap Blocks: exact=204
              Buffers: shared hit=266
              ->  BitmapAnd (actual time=1.198..1.199 rows=0.00 loops=1)
                    Buffers: shared hit=62
                    ->  Bitmap Index Scan on memories_t_<tenant>_claim_fts (actual time=0.199..0.199 rows=404.00 loops=1)
                          Index Cond: (to_tsvector('simple'::regconfig, claim) @@ '''canary'' | ''5'' | ''traffic'''::tsquery)
                          Buffers: shared hit=7
                    ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=0.975..0.975 rows=6522.00 loops=1)
                          Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 3.103 ms
Execution Time: 14.742 ms
```

### b_simple_or_rank@no_rls — long query

```
Limit (actual time=189.552..189.555 rows=10.00 loops=1)
  Buffers: shared hit=3859
  ->  Sort (actual time=189.551..189.552 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=3859
        ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.067..188.699 rows=5903.00 loops=1)
              Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery))
              Rows Removed by Filter: 619
              Buffers: shared hit=3853
Planning Time: 1.386 ms
Execution Time: 189.583 ms
```

### b_simple_or_rank_cd — exact query

```
Limit (actual time=124.264..124.267 rows=10.00 loops=1)
  Buffers: shared hit=3859
  ->  Sort (actual time=124.262..124.264 rows=10.00 loops=1)
        Sort Key: (ts_rank_cd(to_tsvector('simple'::regconfig, m.claim), '''canary'' | ''5'' | ''traffic'''::tsquery, 0)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=3859
        ->  Result (actual time=1.180..124.107 rows=236.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              Buffers: shared hit=3853
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.961..118.149 rows=236.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''canary'' | ''5'' | ''traffic'''::tsquery))
                    Rows Removed by Filter: 6286
                    Buffers: shared hit=3853
Planning Time: 2.124 ms
Execution Time: 124.324 ms
```

### b_simple_or_rank_cd — long query

```
Limit (actual time=319.779..319.783 rows=10.00 loops=1)
  Buffers: shared hit=3859
  ->  Sort (actual time=319.777..319.779 rows=10.00 loops=1)
        Sort Key: (ts_rank_cd(to_tsvector('simple'::regconfig, m.claim), '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=3859
        ->  Result (actual time=2.056..318.247 rows=5903.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              Buffers: shared hit=3853
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.098..109.540 rows=5903.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery))
                    Rows Removed by Filter: 619
                    Buffers: shared hit=3853
Planning Time: 2.778 ms
Execution Time: 319.846 ms
```

### d_bm25_querytime — exact query

```
Limit (actual time=106.529..106.537 rows=10.00 loops=1)
  Buffers: shared hit=7715
  CTE qt
    ->  Aggregate (actual time=0.059..0.061 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Sort (actual time=0.054..0.055 rows=3.00 loops=1)
                Sort Key: x.x
                Sort Method: quicksort  Memory: 25kB
                Buffers: shared hit=3
                ->  Function Scan on unnest x (actual time=0.006..0.007 rows=3.00 loops=1)
  CTE st
    ->  Aggregate (actual time=4.507..4.508 rows=1.00 loops=1)
          Buffers: shared hit=3853
          ->  Result (actual time=0.031..4.162 rows=6096.00 loops=1)
                One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                Buffers: shared hit=3853
                ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.027..3.726 rows=6096.00 loops=1)
                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                      Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])))
                      Rows Removed by Filter: 426
                      Buffers: shared hit=3853
  CTE tf
    ->  Result (actual time=0.560..101.390 rows=284.00 loops=1)
          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
          Buffers: shared hit=3856
          ->  Nested Loop (actual time=0.556..101.338 rows=284.00 loops=1)
                Join Filter: (x_1.lexeme = ANY (qt.terms))
                Rows Removed by Join Filter: 7680
                Buffers: shared hit=3856
                ->  Nested Loop (actual time=0.530..93.908 rows=236.00 loops=1)
                      Buffers: shared hit=3856
                      ->  CTE Scan on qt (actual time=0.060..0.061 rows=1.00 loops=1)
                            Storage: Memory  Maximum Storage: 17kB
                            Buffers: shared hit=3
                      ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=0.468..93.799 rows=236.00 loops=1)
                            Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                            Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''canary'' | ''5'' | ''traffic'''::tsquery))
                            Rows Removed by Filter: 6286
                            Buffers: shared hit=3853
                ->  Function Scan on unnest x_1 (actual time=0.025..0.027 rows=33.75 loops=236)
```

### d_bm25_querytime — long query

```
Limit (actual time=290.722..290.743 rows=10.00 loops=1)
  Buffers: shared hit=7715
  CTE qt
    ->  Aggregate (actual time=0.044..0.045 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Sort (actual time=0.035..0.038 rows=22.00 loops=1)
                Sort Key: x.x
                Sort Method: quicksort  Memory: 25kB
                Buffers: shared hit=3
                ->  Function Scan on unnest x (actual time=0.013..0.014 rows=22.00 loops=1)
  CTE st
    ->  Aggregate (actual time=4.509..4.511 rows=1.00 loops=1)
          Buffers: shared hit=3853
          ->  Result (actual time=0.015..4.164 rows=6096.00 loops=1)
                One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                Buffers: shared hit=3853
                ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.014..3.726 rows=6096.00 loops=1)
                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                      Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])))
                      Rows Removed by Filter: 426
                      Buffers: shared hit=3853
  CTE tf
    ->  Result (actual time=0.146..263.676 rows=16698.00 loops=1)
          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
          Buffers: shared hit=3856
          ->  Nested Loop (actual time=0.143..261.523 rows=16698.00 loops=1)
                Join Filter: (x_1.lexeme = ANY (qt.terms))
                Rows Removed by Join Filter: 143307
                Buffers: shared hit=3856
                ->  Nested Loop (actual time=0.104..99.530 rows=5903.00 loops=1)
                      Buffers: shared hit=3856
                      ->  CTE Scan on qt (actual time=0.045..0.045 rows=1.00 loops=1)
                            Storage: Memory  Maximum Storage: 17kB
                            Buffers: shared hit=3
                      ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=0.059..98.749 rows=5903.00 loops=1)
                            Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                            Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery))
                            Rows Removed by Filter: 619
                            Buffers: shared hit=3853
                ->  Function Scan on unnest x_1 (actual time=0.019..0.020 rows=27.11 loops=5903)
```

### d_bm25_maintained — exact query

```
Limit (actual time=122.580..122.591 rows=10.00 loops=1)
  Buffers: shared hit=4125
  CTE qt
    ->  Aggregate (actual time=0.063..0.065 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Sort (actual time=0.057..0.058 rows=3.00 loops=1)
                Sort Key: x.x
                Sort Method: quicksort  Memory: 25kB
                Buffers: shared hit=3
                ->  Function Scan on unnest x (actual time=0.010..0.011 rows=3.00 loops=1)
  CTE st
    ->  Aggregate (actual time=0.065..0.067 rows=1.00 loops=1)
          Buffers: shared hit=1
          ->  Result (actual time=0.038..0.047 rows=17.00 loops=1)
                One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                Buffers: shared hit=1
                ->  Seq Scan on bm25_scope s (actual time=0.030..0.035 rows=17.00 loops=1)
                      Filter: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                      Buffers: shared hit=1
  CTE df
    ->  GroupAggregate (actual time=0.231..0.243 rows=3.00 loops=1)
          Group Key: d.term
          Buffers: shared hit=47
          ->  Sort (actual time=0.221..0.228 rows=38.00 loops=1)
                Sort Key: d.term
                Sort Method: quicksort  Memory: 26kB
                Buffers: shared hit=47
                ->  Result (actual time=0.049..0.181 rows=38.00 loops=1)
                      One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                      Buffers: shared hit=47
                      ->  Nested Loop (actual time=0.045..0.169 rows=38.00 loops=1)
                            Buffers: shared hit=47
                            ->  CTE Scan on qt (actual time=0.000..0.001 rows=1.00 loops=1)
                                  Storage: Memory  Maximum Storage: 17kB
                            ->  Index Scan using bm25_df_pkey on bm25_df d (actual time=0.042..0.155 rows=38.00 loops=1)
                                  Index Cond: ((tenant_id = '<uuid>'::uuid) AND (term = ANY (qt.terms)) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                  Buffers: shared hit=47
  CTE tf
    ->  Result (actual time=1.346..121.486 rows=294.00 loops=1)
          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
```

### d_bm25_maintained — long query

```
Limit (actual time=284.118..284.131 rows=10.00 loops=1)
  Buffers: shared hit=4418
  CTE qt
    ->  Aggregate (actual time=0.052..0.054 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Sort (actual time=0.041..0.043 rows=22.00 loops=1)
                Sort Key: x.x
                Sort Method: quicksort  Memory: 25kB
                Buffers: shared hit=3
                ->  Function Scan on unnest x (actual time=0.018..0.019 rows=22.00 loops=1)
  CTE st
    ->  Aggregate (actual time=0.034..0.036 rows=1.00 loops=1)
          Buffers: shared hit=1
          ->  Result (actual time=0.015..0.020 rows=17.00 loops=1)
                One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                Buffers: shared hit=1
                ->  Seq Scan on bm25_scope s (actual time=0.012..0.015 rows=17.00 loops=1)
                      Filter: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                      Buffers: shared hit=1
  CTE df
    ->  GroupAggregate (actual time=0.641..0.675 rows=18.00 loops=1)
          Group Key: d.term
          Buffers: shared hit=340
          ->  Sort (actual time=0.638..0.649 rows=283.00 loops=1)
                Sort Key: d.term
                Sort Method: quicksort  Memory: 32kB
                Buffers: shared hit=340
                ->  Result (actual time=0.047..0.596 rows=283.00 loops=1)
                      One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                      Buffers: shared hit=340
                      ->  Nested Loop (actual time=0.046..0.571 rows=283.00 loops=1)
                            Buffers: shared hit=340
                            ->  CTE Scan on qt (actual time=0.000..0.001 rows=1.00 loops=1)
                                  Storage: Memory  Maximum Storage: 17kB
                            ->  Index Scan using bm25_df_pkey on bm25_df d (actual time=0.044..0.542 rows=283.00 loops=1)
                                  Index Cond: ((tenant_id = '<uuid>'::uuid) AND (term = ANY (qt.terms)) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                  Buffers: shared hit=340
  CTE tf
    ->  Result (actual time=0.167..261.100 rows=17536.00 loops=1)
          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
```

### c_dual_or — exact query

```
Limit (actual time=10.555..10.558 rows=10.00 loops=1)
  ->  Sort (actual time=10.553..10.555 rows=10.00 loops=1)
        Sort Key: ((ts_rank(m.fts_english, '''canari'' | ''5'' | ''traffic'''::tsquery, 0) + ts_rank(m.fts_simple, '''canary'' | ''5'' | ''traffic'''::tsquery, 0))) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        ->  Result (actual time=0.095..10.442 rows=254.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.090..10.192 rows=254.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ((fts_english @@ '''canari'' | ''5'' | ''traffic'''::tsquery) OR (fts_simple @@ '''canary'' | ''5'' | ''traffic'''::tsquery)))
                    Rows Removed by Filter: 6868
Planning Time: 2.117 ms
Execution Time: 10.612 ms
```

### c_dual_or — long query

```
Limit (actual time=49.602..49.605 rows=10.00 loops=1)
  ->  Sort (actual time=49.600..49.601 rows=10.00 loops=1)
        Sort Key: ((ts_rank(m.fts_english, '''worker'' | ''kill'' | ''pdf'' | ''render'' | ''job'' | ''roll'' | ''deploy'' | ''invoic'' | ''end'' | ''half'' | ''render'' | ''stop'''::tsquery, 0) + ts_rank(m.fts_simple, '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0))) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        ->  Result (actual time=0.069..48.421 rows=6530.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.045..17.490 rows=6530.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ((fts_english @@ '''worker'' | ''kill'' | ''pdf'' | ''render'' | ''job'' | ''roll'' | ''deploy'' | ''invoic'' | ''end'' | ''half'' | ''render'' | ''stop'''::tsquery) OR (fts_simple @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery)))
                    Rows Removed by Filter: 592
Planning Time: 5.722 ms
Execution Time: 49.698 ms
```

### c_dual_or@no_rls — exact query

```
Limit (actual time=2.750..2.754 rows=10.00 loops=1)
  Buffers: shared hit=362
  ->  Sort (actual time=2.749..2.751 rows=10.00 loops=1)
        Sort Key: ((ts_rank(m.fts_english, '''canari'' | ''5'' | ''traffic'''::tsquery, 0) + ts_rank(m.fts_simple, '''canary'' | ''5'' | ''traffic'''::tsquery, 0))) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=362
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=1.199..2.663 rows=254.00 loops=1)
              Recheck Cond: (((fts_english @@ '''canari'' | ''5'' | ''traffic'''::tsquery) OR (fts_simple @@ '''canary'' | ''5'' | ''traffic'''::tsquery)) AND (tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
              Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])))
              Rows Removed by Filter: 19
              Heap Blocks: exact=253
              Buffers: shared hit=356
              ->  BitmapAnd (actual time=1.147..1.149 rows=0.00 loops=1)
                    Buffers: shared hit=74
                    ->  BitmapOr (actual time=0.198..0.198 rows=0.00 loops=1)
                          Buffers: shared hit=14
                          ->  Bitmap Index Scan on memories_t_<tenant>_fts_english_idx (actual time=0.126..0.126 rows=422.00 loops=1)
                                Index Cond: (fts_english @@ '''canari'' | ''5'' | ''traffic'''::tsquery)
                                Buffers: shared hit=7
                          ->  Bitmap Index Scan on memories_t_<tenant>_fts_simple_idx (actual time=0.071..0.071 rows=422.00 loops=1)
                                Index Cond: (fts_simple @@ '''canary'' | ''5'' | ''traffic'''::tsquery)
                                Buffers: shared hit=7
                    ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=0.913..0.913 rows=7122.00 loops=1)
                          Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                          Buffers: shared hit=60
Planning Time: 2.894 ms
Execution Time: 2.828 ms
```

### c_dual_or@no_rls — long query

```
Limit (actual time=31.628..31.631 rows=10.00 loops=1)
  ->  Sort (actual time=31.627..31.628 rows=10.00 loops=1)
        Sort Key: ((ts_rank(m.fts_english, '''worker'' | ''kill'' | ''pdf'' | ''render'' | ''job'' | ''roll'' | ''deploy'' | ''invoic'' | ''end'' | ''half'' | ''render'' | ''stop'''::tsquery, 0) + ts_rank(m.fts_simple, '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0))) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.027..30.909 rows=6530.00 loops=1)
              Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ((fts_english @@ '''worker'' | ''kill'' | ''pdf'' | ''render'' | ''job'' | ''roll'' | ''deploy'' | ''invoic'' | ''end'' | ''half'' | ''render'' | ''stop'''::tsquery) OR (fts_simple @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery)))
              Rows Removed by Filter: 592
Planning Time: 2.159 ms
Execution Time: 31.651 ms
```

### e_trgm_text — exact query

```
Limit (actual time=208.892..208.894 rows=0.00 loops=1)
  ->  Sort (actual time=208.890..208.891 rows=0.00 loops=1)
        Sort Key: (word_similarity('canary 5% traffic'::text, m.claim)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        ->  Result (actual time=208.817..208.818 rows=0.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=208.811..208.812 rows=0.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('canary 5% traffic'::text <% claim))
                    Rows Removed by Filter: 7422
Planning Time: 3.575 ms
Execution Time: 208.972 ms
```

### e_trgm_text — long query

```
Limit (actual time=342.104..342.105 rows=0.00 loops=1)
  ->  Sort (actual time=342.103..342.104 rows=0.00 loops=1)
        Sort Key: (word_similarity('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text, m.claim)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        ->  Result (actual time=342.065..342.065 rows=0.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=342.061..342.061 rows=0.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text <% claim))
                    Rows Removed by Filter: 7422
Planning Time: 1.843 ms
Execution Time: 342.162 ms
```

### e_trgm_text@no_rls — exact query

```
Limit (actual time=7.796..7.796 rows=0.00 loops=1)
  Buffers: shared hit=149
  ->  Sort (actual time=7.795..7.795 rows=0.00 loops=1)
        Sort Key: (word_similarity('canary 5% traffic'::text, m.claim)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=149
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=7.741..7.742 rows=0.00 loops=1)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('canary 5% traffic'::text <% claim) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 98
              Heap Blocks: exact=97
              Buffers: shared hit=143
              ->  Bitmap Index Scan on memories_t_<tenant>_claim_idx (actual time=1.698..1.698 rows=98.00 loops=1)
                    Index Cond: (claim %> 'canary 5% traffic'::text)
                    Buffers: shared hit=46
Planning Time: 14.769 ms
Execution Time: 7.947 ms
```

### e_trgm_text@no_rls — long query

```
Limit (actual time=12.024..12.025 rows=0.00 loops=1)
  Buffers: shared hit=331
  ->  Sort (actual time=12.023..12.024 rows=0.00 loops=1)
        Sort Key: (word_similarity('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text, m.claim)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=331
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=11.993..11.994 rows=0.00 loops=1)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text <% claim) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 30
              Heap Blocks: exact=30
              Buffers: shared hit=325
              ->  Bitmap Index Scan on memories_t_<tenant>_claim_idx (actual time=10.346..10.347 rows=30.00 loops=1)
                    Index Cond: (claim %> 'The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text)
                    Buffers: shared hit=295
Planning Time: 10.355 ms
Execution Time: 12.116 ms
```

### e_trgm_ident — exact query

```
Limit (actual time=36.178..36.179 rows=0.00 loops=1)
  ->  Sort (actual time=36.176..36.177 rows=0.00 loops=1)
        Sort Key: (word_similarity('canary 5% traffic'::text, m.idents)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        ->  Result (actual time=36.136..36.137 rows=0.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=36.133..36.133 rows=0.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('canary 5% traffic'::text <% idents))
                    Rows Removed by Filter: 7422
Planning Time: 1.731 ms
Execution Time: 36.238 ms
```

### e_trgm_ident — long query

```
Limit (actual time=133.243..133.244 rows=0.00 loops=1)
  ->  Sort (actual time=133.241..133.242 rows=0.00 loops=1)
        Sort Key: (word_similarity('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text, m.idents)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        ->  Result (actual time=133.176..133.177 rows=0.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=133.170..133.171 rows=0.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text <% idents))
                    Rows Removed by Filter: 7422
Planning Time: 3.596 ms
Execution Time: 133.352 ms
```

### e_trgm_ident@no_rls — exact query

```
Limit (actual time=0.301..0.301 rows=0.00 loops=1)
  Buffers: shared hit=43
  ->  Sort (actual time=0.300..0.300 rows=0.00 loops=1)
        Sort Key: (word_similarity('canary 5% traffic'::text, m.idents)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=43
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=0.276..0.276 rows=0.00 loops=1)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('canary 5% traffic'::text <% idents) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Buffers: shared hit=37
              ->  Bitmap Index Scan on memories_t_<tenant>_idents_idx (actual time=0.268..0.268 rows=0.00 loops=1)
                    Index Cond: (idents %> 'canary 5% traffic'::text)
                    Buffers: shared hit=37
Planning Time: 3.071 ms
Execution Time: 0.404 ms
```

### e_trgm_ident@no_rls — long query

```
Limit (actual time=2.012..2.013 rows=0.00 loops=1)
  Buffers: shared hit=198
  ->  Sort (actual time=2.011..2.012 rows=0.00 loops=1)
        Sort Key: (word_similarity('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text, m.idents)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=198
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=1.990..1.991 rows=0.00 loops=1)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text <% idents) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Buffers: shared hit=192
              ->  Bitmap Index Scan on memories_t_<tenant>_idents_idx (actual time=1.981..1.981 rows=0.00 loops=1)
                    Index Cond: (idents %> 'The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text)
                    Buffers: shared hit=192
Planning Time: 5.563 ms
Execution Time: 2.138 ms
```

### f_hnsw — exact query

```
Limit (actual time=0.594..0.614 rows=10.00 loops=1)
  Buffers: shared hit=306
  ->  Result (actual time=0.593..0.612 rows=10.00 loops=1)
        One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
        Buffers: shared hit=306
        ->  Index Scan using memory_vectors_t_<tenant>_hnsw on memory_vectors_t_<tenant> v (actual time=0.588..0.604 rows=10.00 loops=1)
              Order By: (embedding <=> '[…384 floats…]'::vector)
              Filter: (served AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 7
              Buffers: shared hit=306
  Buffers: shared hit=348
Planning Time: 0.721 ms
Execution Time: 0.702 ms
```

### f_hnsw — long query

```
Limit (actual time=1.275..1.371 rows=10.00 loops=1)
  Buffers: shared hit=272
  ->  Result (actual time=1.274..1.368 rows=10.00 loops=1)
        One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
        Buffers: shared hit=272
        ->  Index Scan using memory_vectors_t_<tenant>_hnsw on memory_vectors_t_<tenant> v (actual time=1.269..1.358 rows=10.00 loops=1)
              Order By: (embedding <=> '[…384 floats…]'::vector)
              Filter: (served AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 3
              Buffers: shared hit=272
  Buffers: shared hit=348
Planning Time: 1.121 ms
Execution Time: 1.479 ms
```

## 100k


### a_simple_and — exact query

```
Limit (actual time=538.568..542.920 rows=10.00 loops=1)
  Buffers: shared hit=43957
  ->  Sort (actual time=538.567..542.917 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''pgbouncer'' & ''transaction'' & ''mode'''::tsquery, 0)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=43957
        ->  Gather (actual time=3.138..542.730 rows=242.00 loops=1)
              Workers Planned: 1
              Workers Launched: 1
              Buffers: shared hit=43951
              ->  Parallel Append (actual time=3.373..534.830 rows=121.00 loops=2)
                    Buffers: shared hit=43951
                    ->  Result (actual time=3.372..534.788 rows=121.00 loops=2)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=43951
                          ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=3.312..532.463 rows=121.00 loops=2)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' & ''transaction'' & ''mode'''::tsquery))
                                Rows Removed by Filter: 32325
                                Buffers: shared hit=43951
Planning Time: 1.820 ms
Execution Time: 542.970 ms
```

### a_simple_and — long query

```
Limit (actual time=489.300..494.738 rows=0.00 loops=1)
  Buffers: shared hit=43959
  ->  Sort (actual time=489.299..494.735 rows=0.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery, 0)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=43959
        ->  Gather (actual time=489.246..494.683 rows=0.00 loops=1)
              Workers Planned: 1
              Workers Launched: 1
              Buffers: shared hit=43953
              ->  Parallel Append (actual time=486.836..486.837 rows=0.00 loops=2)
                    Buffers: shared hit=43953
                    ->  Result (actual time=486.834..486.835 rows=0.00 loops=2)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=43953
                          ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=486.830..486.830 rows=0.00 loops=2)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery))
                                Rows Removed by Filter: 32446
                                Buffers: shared hit=43953
Planning Time: 1.749 ms
Execution Time: 494.787 ms
```

### a_simple_and@no_rls — exact query

```
Limit (actual time=6.945..6.947 rows=10.00 loops=1)
  Buffers: shared hit=407
  ->  Sort (actual time=6.943..6.944 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''pgbouncer'' & ''transaction'' & ''mode'''::tsquery, 0)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=407
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=0.386..6.853 rows=242.00 loops=1)
              Recheck Cond: (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' & ''transaction'' & ''mode'''::tsquery)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 159
              Heap Blocks: exact=389
              Buffers: shared hit=401
              ->  Bitmap Index Scan on memories_t_<tenant>_claim_fts (actual time=0.284..0.284 rows=401.00 loops=1)
                    Index Cond: (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' & ''transaction'' & ''mode'''::tsquery)
                    Buffers: shared hit=12
Planning Time: 1.998 ms
Execution Time: 7.030 ms
```

### a_simple_and@no_rls — long query

```
Limit (actual time=0.810..0.811 rows=0.00 loops=1)
  Buffers: shared hit=100
  ->  Sort (actual time=0.809..0.809 rows=0.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery, 0)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=100
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=0.753..0.753 rows=0.00 loops=1)
              Recheck Cond: (to_tsvector('simple'::regconfig, claim) @@ '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Buffers: shared hit=94
              ->  Bitmap Index Scan on memories_t_<tenant>_claim_fts (actual time=0.745..0.746 rows=0.00 loops=1)
                    Index Cond: (to_tsvector('simple'::regconfig, claim) @@ '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery)
                    Buffers: shared hit=94
Planning Time: 1.395 ms
Execution Time: 0.843 ms
```

### b_simple_or_rank — exact query

```
Limit (actual time=508.790..514.392 rows=10.00 loops=1)
  Buffers: shared hit=43973
  ->  Gather Merge (actual time=508.788..514.389 rows=10.00 loops=1)
        Workers Planned: 1
        Workers Launched: 1
        Buffers: shared hit=43973
        ->  Sort (actual time=506.057..506.059 rows=6.00 loops=2)
              Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=43973
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=0.484..505.729 rows=830.50 loops=2)
                    Buffers: shared hit=43958
                    ->  Result (actual time=0.484..505.619 rows=830.50 loops=2)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=43958
                          ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=0.460..492.196 rows=830.50 loops=2)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery))
                                Rows Removed by Filter: 31616
                                Buffers: shared hit=43958
Planning Time: 1.413 ms
Execution Time: 514.443 ms
```

### b_simple_or_rank — long query

```
Limit (actual time=949.206..954.694 rows=10.00 loops=1)
  Buffers: shared hit=43971
  ->  Gather Merge (actual time=949.204..954.691 rows=10.00 loops=1)
        Workers Planned: 1
        Workers Launched: 1
        Buffers: shared hit=43971
        ->  Sort (actual time=946.262..946.264 rows=7.50 loops=2)
              Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=43971
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=0.165..942.369 rows=29308.00 loops=2)
                    Buffers: shared hit=43956
                    ->  Result (actual time=0.165..939.742 rows=29308.00 loops=2)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=43956
                          ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=0.135..487.439 rows=29308.00 loops=2)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery))
                                Rows Removed by Filter: 3138
                                Buffers: shared hit=43956
Planning Time: 1.393 ms
Execution Time: 954.759 ms
```

### b_simple_or_rank@no_rls — exact query

```
Limit (actual time=52.320..52.325 rows=10.00 loops=1)
  Buffers: shared hit=2125
  ->  Sort (actual time=52.318..52.321 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery, 0)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=2125
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=10.129..51.942 rows=1661.00 loops=1)
              Recheck Cond: ((to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery) AND (tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
              Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])))
              Rows Removed by Filter: 142
              Heap Blocks: exact=1573
              Buffers: shared hit=2119
              ->  BitmapAnd (actual time=9.738..9.739 rows=0.00 loops=1)
                    ->  Bitmap Index Scan on memories_t_<tenant>_claim_fts (actual time=1.292..1.292 rows=2772.00 loops=1)
                          Index Cond: (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery)
                          Buffers: shared hit=12
                    ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=8.224..8.225 rows=64892.00 loops=1)
                          Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 2.585 ms
Execution Time: 52.484 ms
```

### b_simple_or_rank@no_rls — long query

```
Limit (actual time=983.721..989.336 rows=10.00 loops=1)
  Buffers: shared hit=43964
  ->  Gather Merge (actual time=983.720..989.332 rows=10.00 loops=1)
        Workers Planned: 1
        Workers Launched: 1
        Buffers: shared hit=43964
        ->  Sort (actual time=979.502..979.503 rows=8.50 loops=2)
              Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=43964
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.173..975.301 rows=29308.00 loops=2)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery))
                    Rows Removed by Filter: 3138
                    Buffers: shared hit=43949
Planning Time: 1.878 ms
Execution Time: 989.396 ms
```

### b_simple_or_rank_cd — exact query

```
Limit (actual time=532.769..539.980 rows=10.00 loops=1)
  Buffers: shared hit=43962
  ->  Gather Merge (actual time=532.768..539.975 rows=10.00 loops=1)
        Workers Planned: 1
        Workers Launched: 1
        Buffers: shared hit=43962
        ->  Sort (actual time=530.224..530.226 rows=8.00 loops=2)
              Sort Key: (ts_rank_cd(to_tsvector('simple'::regconfig, m.claim), '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=43962
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=0.644..529.880 rows=830.50 loops=2)
                    Buffers: shared hit=43947
                    ->  Result (actual time=0.644..529.760 rows=830.50 loops=2)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=43947
                          ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=0.487..513.915 rows=830.50 loops=2)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery))
                                Rows Removed by Filter: 31616
                                Buffers: shared hit=43947
Planning Time: 1.580 ms
Execution Time: 540.046 ms
```

### b_simple_or_rank_cd — long query

```
Limit (actual time=1750.612..1753.550 rows=10.00 loops=1)
  Buffers: shared hit=43962
  ->  Gather Merge (actual time=1750.610..1753.545 rows=10.00 loops=1)
        Workers Planned: 1
        Workers Launched: 1
        Buffers: shared hit=43962
        ->  Sort (actual time=1744.094..1744.095 rows=8.50 loops=2)
              Sort Key: (ts_rank_cd(to_tsvector('simple'::regconfig, m.claim), '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=43962
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=1.102..1736.710 rows=29308.00 loops=2)
                    Buffers: shared hit=43947
                    ->  Result (actual time=1.101..1732.345 rows=29308.00 loops=2)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=43947
                          ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=0.186..603.686 rows=29308.00 loops=2)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery))
                                Rows Removed by Filter: 3138
                                Buffers: shared hit=43947
Planning Time: 1.717 ms
Execution Time: 1753.681 ms
```

### d_bm25_querytime — exact query

```
Limit (actual time=510.106..511.909 rows=10.00 loops=1)
  Buffers: shared hit=87643
  CTE qt
    ->  Aggregate (actual time=0.048..0.049 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Sort (actual time=0.043..0.044 rows=3.00 loops=1)
                Sort Key: x.x
                Sort Method: quicksort  Memory: 25kB
                Buffers: shared hit=3
                ->  Function Scan on unnest x (actual time=0.009..0.009 rows=3.00 loops=1)
  CTE st
    ->  Aggregate (actual time=46.663..46.664 rows=1.00 loops=1)
          Buffers: shared hit=43687
          ->  Result (actual time=0.017..43.379 rows=60440.00 loops=1)
                One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                Buffers: shared hit=43687
                ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.015..39.115 rows=60440.00 loops=1)
                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                      Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])))
                      Rows Removed by Filter: 4452
                      Buffers: shared hit=43687
  CTE tf
    ->  Result (actual time=1.549..461.304 rows=2645.00 loops=1)
          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
          Buffers: shared hit=43950
          ->  Nested Loop (actual time=1.545..460.936 rows=2645.00 loops=1)
                Join Filter: (x_1.lexeme = ANY (qt.terms))
                Rows Removed by Join Filter: 48664
                Buffers: shared hit=43950
                ->  Nested Loop (actual time=1.481..415.624 rows=1661.00 loops=1)
                      Buffers: shared hit=43950
                      ->  CTE Scan on qt (actual time=0.048..0.049 rows=1.00 loops=1)
                            Storage: Memory  Maximum Storage: 17kB
                            Buffers: shared hit=3
                      ->  Gather (actual time=1.431..415.350 rows=1661.00 loops=1)
                            Workers Planned: 1
                            Workers Launched: 1
                            Buffers: shared hit=43947
                            ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=0.914..456.814 rows=830.50 loops=2)
                                  Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
```

### d_bm25_querytime — long query

```
Limit (actual time=2801.829..2801.845 rows=10.00 loops=1)
  Buffers: shared hit=87383
  CTE qt
    ->  Aggregate (actual time=0.087..0.090 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Sort (actual time=0.070..0.074 rows=22.00 loops=1)
                Sort Key: x.x
                Sort Method: quicksort  Memory: 25kB
                Buffers: shared hit=3
                ->  Function Scan on unnest x (actual time=0.026..0.029 rows=22.00 loops=1)
  CTE st
    ->  Aggregate (actual time=70.662..70.663 rows=1.00 loops=1)
          Buffers: shared hit=43687
          ->  Result (actual time=0.035..66.129 rows=60440.00 loops=1)
                One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                Buffers: shared hit=43687
                ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.032..60.519 rows=60440.00 loops=1)
                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                      Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])))
                      Rows Removed by Filter: 4452
                      Buffers: shared hit=43687
  CTE tf
    ->  Result (actual time=0.317..2509.961 rows=165921.00 loops=1)
          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
          Buffers: shared hit=43690
          ->  Nested Loop (actual time=0.308..2489.020 rows=165921.00 loops=1)
                Join Filter: (x_1.lexeme = ANY (qt.terms))
                Rows Removed by Join Filter: 1433953
                Buffers: shared hit=43690
                ->  Nested Loop (actual time=0.227..956.024 rows=58616.00 loops=1)
                      Buffers: shared hit=43690
                      ->  CTE Scan on qt (actual time=0.089..0.089 rows=1.00 loops=1)
                            Storage: Memory  Maximum Storage: 17kB
                            Buffers: shared hit=3
                      ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=0.135..948.985 rows=58616.00 loops=1)
                            Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                            Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery))
                            Rows Removed by Filter: 6276
                            Buffers: shared hit=43687
                ->  Function Scan on unnest x_1 (actual time=0.018..0.019 rows=27.29 loops=58616)
```

### d_bm25_maintained — exact query

```
Limit (actual time=501.731..504.442 rows=10.00 loops=1)
  Buffers: shared hit=44359
  CTE qt
    ->  Aggregate (actual time=0.109..0.111 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Sort (actual time=0.083..0.085 rows=3.00 loops=1)
                Sort Key: x.x
                Sort Method: quicksort  Memory: 25kB
                Buffers: shared hit=3
                ->  Function Scan on unnest x (actual time=0.015..0.015 rows=3.00 loops=1)
  CTE st
    ->  Aggregate (actual time=0.066..0.068 rows=1.00 loops=1)
          Buffers: shared hit=1
          ->  Result (actual time=0.027..0.047 rows=46.00 loops=1)
                One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                Buffers: shared hit=1
                ->  Seq Scan on bm25_scope s (actual time=0.021..0.032 rows=46.00 loops=1)
                      Filter: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                      Buffers: shared hit=1
  CTE df
    ->  GroupAggregate (actual time=1.005..1.045 rows=3.00 loops=1)
          Group Key: d.term
          Buffers: shared hit=145
          ->  Sort (actual time=0.981..1.000 rows=137.00 loops=1)
                Sort Key: d.term
                Sort Method: quicksort  Memory: 29kB
                Buffers: shared hit=145
                ->  Result (actual time=0.095..0.897 rows=137.00 loops=1)
                      One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                      Buffers: shared hit=145
                      ->  Nested Loop (actual time=0.092..0.844 rows=137.00 loops=1)
                            Buffers: shared hit=145
                            ->  CTE Scan on qt (actual time=0.000..0.002 rows=1.00 loops=1)
                                  Storage: Memory  Maximum Storage: 17kB
                            ->  Index Scan using bm25_df_pkey on bm25_df d (actual time=0.088..0.797 rows=137.00 loops=1)
                                  Index Cond: ((tenant_id = '<uuid>'::uuid) AND (term = ANY (qt.terms)) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                  Buffers: shared hit=145
  CTE tf
    ->  Result (actual time=3.685..498.483 rows=2662.00 loops=1)
          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
```

### d_bm25_maintained — long query

```
Limit (actual time=2833.473..2833.488 rows=10.00 loops=1)
  Buffers: shared hit=44842
  CTE qt
    ->  Aggregate (actual time=0.083..0.085 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Sort (actual time=0.067..0.071 rows=22.00 loops=1)
                Sort Key: x.x
                Sort Method: quicksort  Memory: 25kB
                Buffers: shared hit=3
                ->  Function Scan on unnest x (actual time=0.016..0.019 rows=22.00 loops=1)
  CTE st
    ->  Aggregate (actual time=0.072..0.073 rows=1.00 loops=1)
          Buffers: shared hit=1
          ->  Result (actual time=0.033..0.056 rows=46.00 loops=1)
                One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                Buffers: shared hit=1
                ->  Seq Scan on bm25_scope s (actual time=0.030..0.042 rows=46.00 loops=1)
                      Filter: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                      Buffers: shared hit=1
  CTE df
    ->  GroupAggregate (actual time=3.547..3.777 rows=18.00 loops=1)
          Group Key: d.term
          Buffers: shared hit=889
          ->  Sort (actual time=3.528..3.614 rows=827.00 loops=1)
                Sort Key: d.term
                Sort Method: quicksort  Memory: 47kB
                Buffers: shared hit=889
                ->  Result (actual time=0.117..3.277 rows=827.00 loops=1)
                      One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                      Buffers: shared hit=889
                      ->  Nested Loop (actual time=0.115..3.119 rows=827.00 loops=1)
                            Buffers: shared hit=889
                            ->  CTE Scan on qt (actual time=0.000..0.001 rows=1.00 loops=1)
                                  Storage: Memory  Maximum Storage: 17kB
                            ->  Index Scan using bm25_df_pkey on bm25_df d (actual time=0.112..2.927 rows=827.00 loops=1)
                                  Index Cond: ((tenant_id = '<uuid>'::uuid) AND (term = ANY (qt.terms)) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                  Buffers: shared hit=889
  CTE tf
    ->  Result (actual time=0.326..2607.911 rows=166792.00 loops=1)
          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
```

### c_dual_or — exact query

```
Limit (actual time=103.524..103.527 rows=10.00 loops=1)
  ->  Sort (actual time=103.522..103.523 rows=10.00 loops=1)
        Sort Key: ((ts_rank(m.fts_english, '''pgbouncer'' | ''transact'' | ''mode'''::tsquery, 0) + ts_rank(m.fts_simple, '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery, 0))) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        ->  Result (actual time=0.176..103.142 rows=1681.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.165..101.723 rows=1681.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ((fts_english @@ '''pgbouncer'' | ''transact'' | ''mode'''::tsquery) OR (fts_simple @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery)))
                    Rows Removed by Filter: 63811
Planning Time: 5.245 ms
Execution Time: 103.622 ms
```

### c_dual_or — long query

```
Limit (actual time=369.813..369.815 rows=10.00 loops=1)
  ->  Sort (actual time=369.811..369.812 rows=10.00 loops=1)
        Sort Key: ((ts_rank(m.fts_english, '''worker'' | ''kill'' | ''pdf'' | ''render'' | ''job'' | ''roll'' | ''deploy'' | ''invoic'' | ''end'' | ''half'' | ''render'' | ''stop'''::tsquery, 0) + ts_rank(m.fts_simple, '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0))) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        ->  Result (actual time=0.060..360.931 rows=59691.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.048..149.286 rows=59691.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ((fts_english @@ '''worker'' | ''kill'' | ''pdf'' | ''render'' | ''job'' | ''roll'' | ''deploy'' | ''invoic'' | ''end'' | ''half'' | ''render'' | ''stop'''::tsquery) OR (fts_simple @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery)))
                    Rows Removed by Filter: 5801
Planning Time: 2.895 ms
Execution Time: 369.892 ms
```

### c_dual_or@no_rls — exact query

```
Limit (actual time=17.748..17.751 rows=10.00 loops=1)
  Buffers: shared hit=2356
  ->  Sort (actual time=17.747..17.749 rows=10.00 loops=1)
        Sort Key: ((ts_rank(m.fts_english, '''pgbouncer'' | ''transact'' | ''mode'''::tsquery, 0) + ts_rank(m.fts_simple, '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery, 0))) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=2356
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=9.020..17.520 rows=1681.00 loops=1)
              Recheck Cond: (((fts_english @@ '''pgbouncer'' | ''transact'' | ''mode'''::tsquery) OR (fts_simple @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery)) AND (tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
              Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])))
              Rows Removed by Filter: 142
              Heap Blocks: exact=1750
              Buffers: shared hit=2350
              ->  BitmapAnd (actual time=8.852..8.854 rows=0.00 loops=1)
                    ->  BitmapOr (actual time=0.930..0.931 rows=0.00 loops=1)
                          Buffers: shared hit=24
                          ->  Bitmap Index Scan on memories_t_<tenant>_fts_english_idx (actual time=0.602..0.603 rows=2792.00 loops=1)
                                Index Cond: (fts_english @@ '''pgbouncer'' | ''transact'' | ''mode'''::tsquery)
                                Buffers: shared hit=12
                          ->  Bitmap Index Scan on memories_t_<tenant>_fts_simple_idx (actual time=0.327..0.327 rows=2792.00 loops=1)
                                Index Cond: (fts_simple @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery)
                                Buffers: shared hit=12
                    ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=7.718..7.718 rows=65492.00 loops=1)
                          Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 2.170 ms
Execution Time: 17.836 ms
```

### c_dual_or@no_rls — long query

```
Limit (actual time=301.211..301.214 rows=10.00 loops=1)
  ->  Sort (actual time=301.210..301.211 rows=10.00 loops=1)
        Sort Key: ((ts_rank(m.fts_english, '''worker'' | ''kill'' | ''pdf'' | ''render'' | ''job'' | ''roll'' | ''deploy'' | ''invoic'' | ''end'' | ''half'' | ''render'' | ''stop'''::tsquery, 0) + ts_rank(m.fts_simple, '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0))) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.040..294.614 rows=59691.00 loops=1)
              Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ((fts_english @@ '''worker'' | ''kill'' | ''pdf'' | ''render'' | ''job'' | ''roll'' | ''deploy'' | ''invoic'' | ''end'' | ''half'' | ''render'' | ''stop'''::tsquery) OR (fts_simple @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery)))
              Rows Removed by Filter: 5801
Planning Time: 2.104 ms
Execution Time: 301.237 ms
```

### e_trgm_text — exact query

```
Limit (actual time=1832.338..1832.341 rows=10.00 loops=1)
  ->  Sort (actual time=1832.337..1832.338 rows=10.00 loops=1)
        Sort Key: (word_similarity('PgBouncer transaction mode'::text, m.claim)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        ->  Result (actual time=1.510..1831.761 rows=688.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=1.470..1813.188 rows=688.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('PgBouncer transaction mode'::text <% claim))
                    Rows Removed by Filter: 65104
Planning Time: 3.219 ms
Execution Time: 1832.420 ms
```

### e_trgm_text — long query

```
Limit (actual time=3031.164..3031.165 rows=0.00 loops=1)
  ->  Sort (actual time=3031.162..3031.163 rows=0.00 loops=1)
        Sort Key: (word_similarity('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text, m.claim)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        ->  Result (actual time=3031.114..3031.114 rows=0.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=3031.108..3031.108 rows=0.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text <% claim))
                    Rows Removed by Filter: 65792
Planning Time: 3.344 ms
Execution Time: 3031.273 ms
```

### e_trgm_text@no_rls — exact query

```
Limit (actual time=99.504..99.509 rows=10.00 loops=1)
  Buffers: shared hit=1734
  ->  Sort (actual time=99.502..99.506 rows=10.00 loops=1)
        Sort Key: (word_similarity('PgBouncer transaction mode'::text, m.claim)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=1734
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=35.164..99.176 rows=688.00 loops=1)
              Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
              Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('PgBouncer transaction mode'::text <% claim))
              Rows Removed by Filter: 245
              Heap Blocks: exact=919
              Buffers: shared hit=1728
              ->  BitmapAnd (actual time=34.957..34.959 rows=0.00 loops=1)
                    Buffers: shared hit=809
                    ->  Bitmap Index Scan on memories_t_<tenant>_claim_idx (actual time=25.164..25.164 rows=1436.00 loops=1)
                          Index Cond: (claim %> 'PgBouncer transaction mode'::text)
                          Buffers: shared hit=269
                    ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=9.525..9.525 rows=65792.00 loops=1)
                          Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 13.948 ms
Execution Time: 99.751 ms
```

### e_trgm_text@no_rls — long query

```
Limit (actual time=119.213..119.215 rows=0.00 loops=1)
  Buffers: shared hit=1155
  ->  Sort (actual time=119.211..119.212 rows=0.00 loops=1)
        Sort Key: (word_similarity('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text, m.claim)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=1155
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=119.163..119.164 rows=0.00 loops=1)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text <% claim) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 225
              Heap Blocks: exact=225
              Buffers: shared hit=1149
              ->  Bitmap Index Scan on memories_t_<tenant>_claim_idx (actual time=104.935..104.936 rows=225.00 loops=1)
                    Index Cond: (claim %> 'The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text)
                    Buffers: shared hit=924
Planning Time: 22.112 ms
Execution Time: 119.568 ms
```

### e_trgm_ident — exact query

```
Limit (actual time=406.131..406.132 rows=0.00 loops=1)
  ->  Sort (actual time=406.129..406.130 rows=0.00 loops=1)
        Sort Key: (word_similarity('PgBouncer transaction mode'::text, m.idents)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        ->  Result (actual time=406.079..406.080 rows=0.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=406.072..406.072 rows=0.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('PgBouncer transaction mode'::text <% idents))
                    Rows Removed by Filter: 65792
Planning Time: 3.627 ms
Execution Time: 406.237 ms
```

### e_trgm_ident — long query

```
Limit (actual time=1032.898..1032.900 rows=0.00 loops=1)
  ->  Sort (actual time=1032.897..1032.898 rows=0.00 loops=1)
        Sort Key: (word_similarity('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text, m.idents)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        ->  Result (actual time=1032.868..1032.868 rows=0.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=1032.864..1032.865 rows=0.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text <% idents))
                    Rows Removed by Filter: 65792
Planning Time: 1.966 ms
Execution Time: 1032.959 ms
```

### e_trgm_ident@no_rls — exact query

```
Limit (actual time=6.176..6.178 rows=0.00 loops=1)
  Buffers: shared hit=147
  ->  Sort (actual time=6.175..6.176 rows=0.00 loops=1)
        Sort Key: (word_similarity('PgBouncer transaction mode'::text, m.idents)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=147
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=6.132..6.134 rows=0.00 loops=1)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('PgBouncer transaction mode'::text <% idents) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Buffers: shared hit=141
              ->  Bitmap Index Scan on memories_t_<tenant>_idents_idx (actual time=6.120..6.121 rows=0.00 loops=1)
                    Index Cond: (idents %> 'PgBouncer transaction mode'::text)
                    Buffers: shared hit=141
Planning Time: 6.030 ms
Execution Time: 6.454 ms
```

### e_trgm_ident@no_rls — long query

```
Limit (actual time=15.729..15.730 rows=0.00 loops=1)
  Buffers: shared hit=499
  ->  Sort (actual time=15.727..15.728 rows=0.00 loops=1)
        Sort Key: (word_similarity('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text, m.idents)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=499
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=15.707..15.708 rows=0.00 loops=1)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text <% idents) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Buffers: shared hit=493
              ->  Bitmap Index Scan on memories_t_<tenant>_idents_idx (actual time=15.695..15.696 rows=0.00 loops=1)
                    Index Cond: (idents %> 'The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text)
                    Buffers: shared hit=493
Planning Time: 4.745 ms
Execution Time: 15.959 ms
```

### f_hnsw — exact query

```
Limit (actual time=2.542..2.686 rows=10.00 loops=1)
  Buffers: shared hit=601
  ->  Result (actual time=2.541..2.683 rows=10.00 loops=1)
        One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
        Buffers: shared hit=601
        ->  Index Scan using memory_vectors_t_<tenant>_hnsw on memory_vectors_t_<tenant> v (actual time=2.534..2.670 rows=10.00 loops=1)
              Order By: (embedding <=> '[…384 floats…]'::vector)
              Filter: (served AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 8
              Buffers: shared hit=601
  Buffers: shared hit=348
Planning Time: 1.470 ms
Execution Time: 2.784 ms
```

### f_hnsw — long query

```
Limit (actual time=3.654..3.777 rows=10.00 loops=1)
  ->  Result (actual time=3.654..3.775 rows=10.00 loops=1)
        One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
        ->  Index Scan using memory_vectors_t_<tenant>_hnsw on memory_vectors_t_<tenant> v (actual time=3.650..3.768 rows=10.00 loops=1)
              Order By: (embedding <=> '[…384 floats…]'::vector)
              Filter: (served AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 10
  Buffers: shared hit=348
Planning Time: 0.674 ms
Execution Time: 3.842 ms
```

## 1M


### c_dual_or — exact query

```
Limit (actual time=639.743..694.503 rows=10.00 loops=1)
  Buffers: shared hit=631147
  ->  Gather Merge (actual time=639.742..694.499 rows=10.00 loops=1)
        Workers Planned: 1
        Workers Launched: 1
        Buffers: shared hit=631147
        ->  Sort (actual time=636.276..636.277 rows=7.00 loops=2)
              Sort Key: ((ts_rank(m.fts_english, '''idempotency-key'' | ''idempot'' | ''key'' | ''header'''::tsquery, 0) + ts_rank(m.fts_simple, '''idempotency-key'' | ''idempotency'' | ''key'' | ''header'''::tsquery, 0))) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=631147
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=0.134..632.053 rows=37632.00 loops=2)
                    Buffers: shared hit=631132
                    ->  Result (actual time=0.133..629.136 rows=37632.00 loops=2)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=631132
                          ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=0.126..601.793 rows=37632.00 loops=2)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ((fts_english @@ '''idempotency-key'' | ''idempot'' | ''key'' | ''header'''::tsquery) OR (fts_simple @@ '''idempotency-key'' | ''idempotency'' | ''key'' | ''header'''::tsquery)))
                                Rows Removed by Filter: 287662
                                Buffers: shared hit=629657
Planning Time: 2.411 ms
Execution Time: 694.600 ms
```

### c_dual_or — long query

```
Limit (actual time=1645.177..1698.953 rows=10.00 loops=1)
  Buffers: shared hit=632489
  ->  Gather Merge (actual time=1645.176..1698.949 rows=10.00 loops=1)
        Workers Planned: 1
        Workers Launched: 1
        Buffers: shared hit=632489
        ->  Sort (actual time=1639.528..1639.529 rows=6.50 loops=2)
              Sort Key: ((ts_rank(m.fts_english, '''partner'' | ''integr'' | ''hammer'' | ''invoic'' | ''list'' | ''endpoint'' | ''get'' | ''rate'' | ''limit'' | ''limit'' | ''use'' | ''instead'''::tsquery, 0) + ts_rank(m.fts_simple, '''a'' | ''partner'' | ''integration'' | ''is'' | ''hammering'' | ''the'' | ''invoices'' | ''list'' | ''endpoint'' | ''and'' | ''getting'' | ''rate'' | ''limited'' | ''what'' | ''is'' | ''the'' | ''limit'' | ''and'' | ''what'' | ''should'' | ''they'' | ''use'' | ''instead'''::tsquery, 0))) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=632489
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=0.213..1607.199 rows=298208.00 loops=2)
                    Buffers: shared hit=632474
                    ->  Result (actual time=0.212..1583.475 rows=298208.00 loops=2)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=632474
                          ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=0.181..735.721 rows=298208.00 loops=2)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ((fts_english @@ '''partner'' | ''integr'' | ''hammer'' | ''invoic'' | ''list'' | ''endpoint'' | ''get'' | ''rate'' | ''limit'' | ''limit'' | ''use'' | ''instead'''::tsquery) OR (fts_simple @@ '''a'' | ''partner'' | ''integration'' | ''is'' | ''hammering'' | ''the'' | ''invoices'' | ''list'' | ''endpoint'' | ''and'' | ''getting'' | ''rate'' | ''limited'' | ''what'' | ''is'' | ''the'' | ''limit'' | ''and'' | ''what'' | ''should'' | ''they'' | ''use'' | ''instead'''::tsquery)))
                                Rows Removed by Filter: 27086
                                Buffers: shared hit=628003
Planning Time: 2.622 ms
Execution Time: 1699.040 ms
```

### c_dual_or@no_rls — exact query

```
Limit (actual time=289.875..340.014 rows=10.00 loops=1)
  Buffers: shared hit=71979
  ->  Gather Merge (actual time=289.873..340.010 rows=10.00 loops=1)
        Workers Planned: 2
        Workers Launched: 2
        Buffers: shared hit=71979
        ->  Sort (actual time=282.578..282.580 rows=7.33 loops=3)
              Sort Key: ((ts_rank(m.fts_english, '''idempotency-key'' | ''idempot'' | ''key'' | ''header'''::tsquery, 0) + ts_rank(m.fts_simple, '''idempotency-key'' | ''idempotency'' | ''key'' | ''header'''::tsquery, 0))) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=71979
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              Worker 1:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m (actual time=140.487..278.973 rows=25088.00 loops=3)
                    Recheck Cond: (((fts_english @@ '''idempotency-key'' | ''idempot'' | ''key'' | ''header'''::tsquery) OR (fts_simple @@ '''idempotency-key'' | ''idempotency'' | ''key'' | ''header'''::tsquery)) AND (tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])))
                    Rows Removed by Filter: 1890
                    Heap Blocks: exact=22853
                    Buffers: shared hit=71949
                    Worker 0:  Heap Blocks: exact=19140
                    Worker 1:  Heap Blocks: exact=22913
                    ->  BitmapAnd (actual time=135.341..135.343 rows=0.00 loops=1)
                          ->  BitmapOr (actual time=49.216..49.217 rows=0.00 loops=1)
                                Buffers: shared hit=107
                                ->  Bitmap Index Scan on memories_t_<tenant>_fts_english_idx (actual time=35.655..35.655 rows=124571.00 loops=1)
                                      Index Cond: (fts_english @@ '''idempotency-key'' | ''idempot'' | ''key'' | ''header'''::tsquery)
                                ->  Bitmap Index Scan on memories_t_<tenant>_fts_simple_idx (actual time=13.559..13.559 rows=83451.00 loops=1)
                                      Index Cond: (fts_simple @@ '''idempotency-key'' | ''idempotency'' | ''key'' | ''header'''::tsquery)
                                      Buffers: shared hit=49
                          ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=82.219..82.219 rows=650587.00 loops=1)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 3.572 ms
Execution Time: 340.168 ms
```

### c_dual_or@no_rls — long query

```
Limit (actual time=1560.157..1616.973 rows=10.00 loops=1)
  Buffers: shared hit=632487
  ->  Gather Merge (actual time=1560.155..1616.968 rows=10.00 loops=1)
        Workers Planned: 1
        Workers Launched: 1
        Buffers: shared hit=632487
        ->  Sort (actual time=1556.661..1556.663 rows=7.50 loops=2)
              Sort Key: ((ts_rank(m.fts_english, '''partner'' | ''integr'' | ''hammer'' | ''invoic'' | ''list'' | ''endpoint'' | ''get'' | ''rate'' | ''limit'' | ''limit'' | ''use'' | ''instead'''::tsquery, 0) + ts_rank(m.fts_simple, '''a'' | ''partner'' | ''integration'' | ''is'' | ''hammering'' | ''the'' | ''invoices'' | ''list'' | ''endpoint'' | ''and'' | ''getting'' | ''rate'' | ''limited'' | ''what'' | ''is'' | ''the'' | ''limit'' | ''and'' | ''what'' | ''should'' | ''they'' | ''use'' | ''instead'''::tsquery, 0))) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=632487
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.117..1525.970 rows=298208.00 loops=2)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ((fts_english @@ '''partner'' | ''integr'' | ''hammer'' | ''invoic'' | ''list'' | ''endpoint'' | ''get'' | ''rate'' | ''limit'' | ''limit'' | ''use'' | ''instead'''::tsquery) OR (fts_simple @@ '''a'' | ''partner'' | ''integration'' | ''is'' | ''hammering'' | ''the'' | ''invoices'' | ''list'' | ''endpoint'' | ''and'' | ''getting'' | ''rate'' | ''limited'' | ''what'' | ''is'' | ''the'' | ''limit'' | ''and'' | ''what'' | ''should'' | ''they'' | ''use'' | ''instead'''::tsquery)))
                    Rows Removed by Filter: 27086
                    Buffers: shared hit=632472
Planning Time: 2.446 ms
Execution Time: 1617.052 ms
```

### a_stored_and — exact query

```
Limit (actual time=557.932..624.512 rows=10.00 loops=1)
  Buffers: shared hit=631155
  ->  Gather Merge (actual time=557.930..624.507 rows=10.00 loops=1)
        Workers Planned: 1
        Workers Launched: 1
        Buffers: shared hit=631155
        ->  Sort (actual time=551.801..551.803 rows=8.00 loops=2)
              Sort Key: (ts_rank(m.fts_simple, '''idempotency-key'' & ''idempotency'' & ''key'' & ''header'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=631155
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=1.913..551.093 rows=1811.50 loops=2)
                    Buffers: shared hit=631140
                    ->  Result (actual time=1.912..550.817 rows=1811.50 loops=2)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=631140
                          ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=1.848..548.911 rows=1811.50 loops=2)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (fts_simple @@ '''idempotency-key'' & ''idempotency'' & ''key'' & ''header'''::tsquery))
                                Rows Removed by Filter: 323482
                                Buffers: shared hit=631134
Planning Time: 2.083 ms
Execution Time: 624.587 ms
```

### a_stored_and — long query

```
Limit (actual time=576.787..640.139 rows=0.00 loops=1)
  Buffers: shared hit=631141
  ->  Gather Merge (actual time=576.786..640.135 rows=0.00 loops=1)
        Workers Planned: 1
        Workers Launched: 1
        Buffers: shared hit=631141
        ->  Sort (actual time=571.367..571.368 rows=0.00 loops=2)
              Sort Key: (ts_rank(m.fts_simple, '''a'' & ''partner'' & ''integration'' & ''is'' & ''hammering'' & ''the'' & ''invoices'' & ''list'' & ''endpoint'' & ''and'' & ''getting'' & ''rate'' & ''limited'' & ''what'' & ''is'' & ''the'' & ''limit'' & ''and'' & ''what'' & ''should'' & ''they'' & ''use'' & ''instead'''::tsquery, 0)) DESC, m.id
              Sort Method: quicksort  Memory: 25kB
              Buffers: shared hit=631141
              Worker 0:  Sort Method: quicksort  Memory: 25kB
              ->  Parallel Append (actual time=571.311..571.312 rows=0.00 loops=2)
                    Buffers: shared hit=631126
                    ->  Result (actual time=571.309..571.310 rows=0.00 loops=2)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=631126
                          ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=571.300..571.300 rows=0.00 loops=2)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (fts_simple @@ '''a'' & ''partner'' & ''integration'' & ''is'' & ''hammering'' & ''the'' & ''invoices'' & ''list'' & ''endpoint'' & ''and'' & ''getting'' & ''rate'' & ''limited'' & ''what'' & ''is'' & ''the'' & ''limit'' & ''and'' & ''what'' & ''should'' & ''they'' & ''use'' & ''instead'''::tsquery))
                                Rows Removed by Filter: 325294
                                Buffers: shared hit=631126
Planning Time: 1.844 ms
Execution Time: 640.284 ms
```

### a_stored_and@no_rls — exact query

```
Limit (actual time=55.415..55.418 rows=10.00 loops=1)
  ->  Sort (actual time=55.413..55.415 rows=10.00 loops=1)
        Sort Key: (ts_rank(m.fts_simple, '''idempotency-key'' & ''idempotency'' & ''key'' & ''header'''::tsquery, 0)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=8.576..54.607 rows=3623.00 loops=1)
              Recheck Cond: (fts_simple @@ '''idempotency-key'' & ''idempotency'' & ''key'' & ''header'''::tsquery)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 2239
              Heap Blocks: exact=5778
              ->  Bitmap Index Scan on memories_t_<tenant>_fts_simple_idx (actual time=6.999..7.000 rows=5862.00 loops=1)
                    Index Cond: (fts_simple @@ '''idempotency-key'' & ''idempotency'' & ''key'' & ''header'''::tsquery)
                    Buffers: shared hit=75
Planning Time: 4.329 ms
Execution Time: 55.540 ms
```

### a_stored_and@no_rls — long query

```
Limit (actual time=1.091..1.092 rows=0.00 loops=1)
  Buffers: shared hit=112
  ->  Sort (actual time=1.090..1.091 rows=0.00 loops=1)
        Sort Key: (ts_rank(m.fts_simple, '''a'' & ''partner'' & ''integration'' & ''is'' & ''hammering'' & ''the'' & ''invoices'' & ''list'' & ''endpoint'' & ''and'' & ''getting'' & ''rate'' & ''limited'' & ''what'' & ''is'' & ''the'' & ''limit'' & ''and'' & ''what'' & ''should'' & ''they'' & ''use'' & ''instead'''::tsquery, 0)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=112
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=1.061..1.062 rows=0.00 loops=1)
              Recheck Cond: (fts_simple @@ '''a'' & ''partner'' & ''integration'' & ''is'' & ''hammering'' & ''the'' & ''invoices'' & ''list'' & ''endpoint'' & ''and'' & ''getting'' & ''rate'' & ''limited'' & ''what'' & ''is'' & ''the'' & ''limit'' & ''and'' & ''what'' & ''should'' & ''they'' & ''use'' & ''instead'''::tsquery)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Buffers: shared hit=106
              ->  Bitmap Index Scan on memories_t_<tenant>_fts_simple_idx (actual time=1.054..1.055 rows=0.00 loops=1)
                    Index Cond: (fts_simple @@ '''a'' & ''partner'' & ''integration'' & ''is'' & ''hammering'' & ''the'' & ''invoices'' & ''list'' & ''endpoint'' & ''and'' & ''getting'' & ''rate'' & ''limited'' & ''what'' & ''is'' & ''the'' & ''limit'' & ''and'' & ''what'' & ''should'' & ''they'' & ''use'' & ''instead'''::tsquery)
                    Buffers: shared hit=106
Planning Time: 1.911 ms
Execution Time: 1.132 ms
```

### b_stored_or — exact query

```
Limit (actual time=563.725..623.783 rows=10.00 loops=1)
  Buffers: shared hit=632328
  ->  Gather Merge (actual time=563.723..623.779 rows=10.00 loops=1)
        Workers Planned: 1
        Workers Launched: 1
        Buffers: shared hit=632328
        ->  Sort (actual time=558.404..558.406 rows=8.00 loops=2)
              Sort Key: (ts_rank(m.fts_simple, '''idempotency-key'' | ''idempotency'' | ''key'' | ''header'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=632328
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=0.346..555.031 rows=25222.50 loops=2)
                    Buffers: shared hit=632313
                    ->  Result (actual time=0.346..552.977 rows=25222.50 loops=2)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=632313
                          ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=0.335..541.619 rows=25222.50 loops=2)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (fts_simple @@ '''idempotency-key'' | ''idempotency'' | ''key'' | ''header'''::tsquery))
                                Rows Removed by Filter: 300071
                                Buffers: shared hit=631128
Planning Time: 1.673 ms
Execution Time: 623.862 ms
```

### b_stored_or — long query

```
Limit (actual time=1254.110..1311.457 rows=10.00 loops=1)
  Buffers: shared hit=635580
  ->  Gather Merge (actual time=1254.109..1311.453 rows=10.00 loops=1)
        Workers Planned: 1
        Workers Launched: 1
        Buffers: shared hit=635580
        ->  Sort (actual time=1247.835..1247.836 rows=8.50 loops=2)
              Sort Key: (ts_rank(m.fts_simple, '''a'' | ''partner'' | ''integration'' | ''is'' | ''hammering'' | ''the'' | ''invoices'' | ''list'' | ''endpoint'' | ''and'' | ''getting'' | ''rate'' | ''limited'' | ''what'' | ''is'' | ''the'' | ''limit'' | ''and'' | ''what'' | ''should'' | ''they'' | ''use'' | ''instead'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=635580
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=0.190..1213.965 rows=296828.00 loops=2)
                    Buffers: shared hit=635565
                    ->  Result (actual time=0.190..1188.901 rows=296828.00 loops=2)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=635565
                          ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=0.171..597.316 rows=296828.00 loops=2)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (fts_simple @@ '''a'' | ''partner'' | ''integration'' | ''is'' | ''hammering'' | ''the'' | ''invoices'' | ''list'' | ''endpoint'' | ''and'' | ''getting'' | ''rate'' | ''limited'' | ''what'' | ''is'' | ''the'' | ''limit'' | ''and'' | ''what'' | ''should'' | ''they'' | ''use'' | ''instead'''::tsquery))
                                Rows Removed by Filter: 28466
                                Buffers: shared hit=631134
Planning Time: 1.920 ms
Execution Time: 1311.568 ms
```

### d_bm25_stored — exact query

```
Limit (actual time=1737.973..1737.989 rows=10.00 loops=1)
  Buffers: shared hit=633894
  CTE qt
    ->  Aggregate (actual time=0.038..0.040 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Sort (actual time=0.032..0.033 rows=4.00 loops=1)
                Sort Key: x.x
                Sort Method: quicksort  Memory: 25kB
                Buffers: shared hit=3
                ->  Function Scan on unnest x (actual time=0.010..0.011 rows=4.00 loops=1)
  CTE st
    ->  Aggregate (actual time=0.158..0.160 rows=1.00 loops=1)
          ->  Result (actual time=0.042..0.128 rows=451.00 loops=1)
                One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                ->  Seq Scan on bm25_scope s (actual time=0.040..0.090 rows=451.00 loops=1)
                      Filter: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
  CTE df
    ->  GroupAggregate (actual time=3.149..3.303 rows=4.00 loops=1)
          Group Key: d.term
          Buffers: shared hit=1812
          ->  Sort (actual time=3.098..3.162 rows=1779.00 loops=1)
                Sort Key: d.term
                Sort Method: quicksort  Memory: 104kB
                Buffers: shared hit=1812
                ->  Result (actual time=0.109..2.875 rows=1779.00 loops=1)
                      One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                      Buffers: shared hit=1812
                      ->  Nested Loop (actual time=0.108..2.742 rows=1779.00 loops=1)
                            Buffers: shared hit=1812
                            ->  CTE Scan on qt (actual time=0.000..0.001 rows=1.00 loops=1)
                                  Storage: Memory  Maximum Storage: 17kB
                            ->  Index Scan using bm25_df_pkey on bm25_df d (actual time=0.106..2.565 rows=1779.00 loops=1)
                                  Index Cond: ((tenant_id = '<uuid>'::uuid) AND (term = ANY (qt.terms)) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                  Buffers: shared hit=1812
  CTE tf
    ->  Result (actual time=0.174..1607.720 rows=69773.00 loops=1)
          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
          Buffers: shared hit=632071
          ->  Nested Loop (actual time=0.169..1597.726 rows=69773.00 loops=1)
                Join Filter: (x_1.lexeme = ANY (qt_1.terms))
```

### d_bm25_stored — long query

```
Limit (actual time=12097.609..12097.627 rows=10.00 loops=1)
  Buffers: shared hit=642202, temp read=5957 written=17841
  CTE qt
    ->  Aggregate (actual time=0.046..0.048 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Sort (actual time=0.032..0.035 rows=19.00 loops=1)
                Sort Key: x.x
                Sort Method: quicksort  Memory: 25kB
                Buffers: shared hit=3
                ->  Function Scan on unnest x (actual time=0.008..0.010 rows=19.00 loops=1)
  CTE st
    ->  Aggregate (actual time=0.207..0.209 rows=1.00 loops=1)
          ->  Result (actual time=0.041..0.169 rows=451.00 loops=1)
                One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                ->  Seq Scan on bm25_scope s (actual time=0.037..0.117 rows=451.00 loops=1)
                      Filter: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
  CTE df
    ->  GroupAggregate (actual time=18.847..19.846 rows=15.00 loops=1)
          Group Key: d.term
          Buffers: shared hit=6874
          ->  Sort (actual time=18.760..19.100 rows=6747.00 loops=1)
                Sort Key: d.term
                Sort Method: quicksort  Memory: 386kB
                Buffers: shared hit=6874
                ->  Result (actual time=0.179..17.547 rows=6747.00 loops=1)
                      One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                      Buffers: shared hit=6874
                      ->  Nested Loop (actual time=0.175..16.895 rows=6747.00 loops=1)
                            Buffers: shared hit=6874
                            ->  CTE Scan on qt (actual time=0.000..0.003 rows=1.00 loops=1)
                                  Storage: Memory  Maximum Storage: 17kB
                            ->  Index Scan using bm25_df_pkey on bm25_df d (actual time=0.173..16.016 rows=6747.00 loops=1)
                                  Index Cond: ((tenant_id = '<uuid>'::uuid) AND (term = ANY (qt.terms)) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                  Buffers: shared hit=6874
  CTE tf
    ->  Result (actual time=0.218..9360.718 rows=1557985.00 loops=1)
          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
          Buffers: shared hit=635317
          ->  Nested Loop (actual time=0.211..9150.330 rows=1557985.00 loops=1)
                Join Filter: (x_1.lexeme = ANY (qt_1.terms))
```

### e_trgm_text — exact query

```
Limit (actual time=8425.406..8485.418 rows=10.00 loops=1)
  Buffers: shared hit=626930 read=23
  ->  Gather Merge (actual time=8425.404..8485.413 rows=10.00 loops=1)
        Workers Planned: 1
        Workers Launched: 1
        Buffers: shared hit=626930 read=23
        ->  Sort (actual time=8419.013..8419.015 rows=6.50 loops=2)
              Sort Key: (word_similarity('Idempotency-Key header'::text, m.claim)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=626930 read=23
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=3.750..8416.308 rows=7048.50 loops=2)
                    Buffers: shared hit=626915 read=23
                    ->  Result (actual time=3.749..8415.296 rows=7048.50 loops=2)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=626915 read=23
                          ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=3.694..8267.948 rows=7048.50 loops=2)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('Idempotency-Key header'::text <% claim))
                                Rows Removed by Filter: 318395
                                Buffers: shared hit=626915 read=23
Planning Time: 3.849 ms
Execution Time: 8485.547 ms
```

### e_trgm_text — long query

```
(timed out)
```

### e_trgm_text@no_rls — exact query

```
Limit (actual time=1024.248..1024.252 rows=10.00 loops=1)
  Buffers: shared hit=22697
  ->  Sort (actual time=1024.246..1024.248 rows=10.00 loops=1)
        Sort Key: (word_similarity('Idempotency-Key header'::text, m.claim)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=22697
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=221.128..1021.967 rows=14097.00 loops=1)
              Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
              Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('Idempotency-Key header'::text <% claim))
              Rows Removed by Filter: 2701
              Heap Blocks: exact=16000
              Buffers: shared hit=22691
              ->  BitmapAnd (actual time=219.131..219.133 rows=0.00 loops=1)
                    Buffers: shared hit=6691
                    ->  Bitmap Index Scan on memories_t_<tenant>_claim_idx (actual time=129.425..129.425 rows=25794.00 loops=1)
                          Index Cond: (claim %> 'Idempotency-Key header'::text)
                          Buffers: shared hit=1356
                    ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=87.153..87.153 rows=650887.00 loops=1)
                          Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 9.630 ms
Execution Time: 1024.483 ms
```

### e_trgm_text@no_rls — long query

```
Limit (actual time=1170.560..1170.561 rows=0.00 loops=1)
  Buffers: shared hit=7916 read=1
  ->  Sort (actual time=1170.558..1170.559 rows=0.00 loops=1)
        Sort Key: (word_similarity('A partner integration is hammering the invoices list endpoint and getting rate limited, what is the limit and what should they use instead?'::text, m.claim)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=7916 read=1
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=1170.503..1170.504 rows=0.00 loops=1)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('A partner integration is hammering the invoices list endpoint and getting rate limited, what is the limit and what should they use instead?'::text <% claim) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 857
              Heap Blocks: exact=856
              Buffers: shared hit=7910 read=1
              ->  Bitmap Index Scan on memories_t_<tenant>_claim_idx (actual time=1119.389..1119.389 rows=857.00 loops=1)
                    Index Cond: (claim %> 'A partner integration is hammering the invoices list endpoint and getting rate limited, what is the limit and what should they use instead?'::text)
                    Buffers: shared hit=7055
Planning Time: 26.508 ms
Execution Time: 1170.784 ms
```

### e_trgm_ident — exact query

```
Limit (actual time=1873.544..1932.668 rows=9.00 loops=1)
  Buffers: shared hit=626958
  ->  Gather Merge (actual time=1873.542..1932.664 rows=9.00 loops=1)
        Workers Planned: 1
        Workers Launched: 1
        Buffers: shared hit=626958
        ->  Sort (actual time=1866.929..1866.930 rows=4.50 loops=2)
              Sort Key: (word_similarity('Idempotency-Key header'::text, m.idents)) DESC, m.id
              Sort Method: quicksort  Memory: 25kB
              Buffers: shared hit=626958
              Worker 0:  Sort Method: quicksort  Memory: 25kB
              ->  Parallel Append (actual time=240.504..1866.852 rows=4.50 loops=2)
                    Buffers: shared hit=626943
                    ->  Result (actual time=240.503..1866.845 rows=4.50 loops=2)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=626943
                          ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=240.483..1866.787 rows=4.50 loops=2)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('Idempotency-Key header'::text <% idents))
                                Rows Removed by Filter: 325439
                                Buffers: shared hit=626943
Planning Time: 4.220 ms
Execution Time: 1932.897 ms
```

### e_trgm_ident — long query

```
Limit (actual time=5904.326..5957.998 rows=0.00 loops=1)
  Buffers: shared hit=626958
  ->  Gather Merge (actual time=5904.324..5957.995 rows=0.00 loops=1)
        Workers Planned: 1
        Workers Launched: 1
        Buffers: shared hit=626958
        ->  Sort (actual time=5899.947..5899.949 rows=0.00 loops=2)
              Sort Key: (word_similarity('A partner integration is hammering the invoices list endpoint and getting rate limited, what is the limit and what should they use instead?'::text, m.idents)) DESC, m.id
              Sort Method: quicksort  Memory: 25kB
              Buffers: shared hit=626958
              Worker 0:  Sort Method: quicksort  Memory: 25kB
              ->  Parallel Append (actual time=5899.916..5899.917 rows=0.00 loops=2)
                    Buffers: shared hit=626943
                    ->  Result (actual time=5899.914..5899.915 rows=0.00 loops=2)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=626943
                          ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=5899.906..5899.907 rows=0.00 loops=2)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('A partner integration is hammering the invoices list endpoint and getting rate limited, what is the limit and what should they use instead?'::text <% idents))
                                Rows Removed by Filter: 325444
                                Buffers: shared hit=626943
Planning Time: 1.885 ms
Execution Time: 5958.068 ms
```

### e_trgm_ident@no_rls — exact query

```
Limit (actual time=34.635..34.639 rows=9.00 loops=1)
  Buffers: shared hit=446
  ->  Sort (actual time=34.633..34.635 rows=9.00 loops=1)
        Sort Key: (word_similarity('Idempotency-Key header'::text, m.idents)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=446
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=33.736..34.577 rows=9.00 loops=1)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('Idempotency-Key header'::text <% idents) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 32
              Heap Blocks: exact=41
              Buffers: shared hit=440
              ->  Bitmap Index Scan on memories_t_<tenant>_idents_idx (actual time=33.546..33.546 rows=41.00 loops=1)
                    Index Cond: (idents %> 'Idempotency-Key header'::text)
                    Buffers: shared hit=399
Planning Time: 7.203 ms
Execution Time: 34.914 ms
```

### e_trgm_ident@no_rls — long query

```
Limit (actual time=165.993..165.995 rows=0.00 loops=1)
  Buffers: shared hit=2282
  ->  Sort (actual time=165.991..165.993 rows=0.00 loops=1)
        Sort Key: (word_similarity('A partner integration is hammering the invoices list endpoint and getting rate limited, what is the limit and what should they use instead?'::text, m.idents)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=2282
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=165.958..165.959 rows=0.00 loops=1)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('A partner integration is hammering the invoices list endpoint and getting rate limited, what is the limit and what should they use instead?'::text <% idents) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Buffers: shared hit=2276
              ->  Bitmap Index Scan on memories_t_<tenant>_idents_idx (actual time=165.945..165.946 rows=0.00 loops=1)
                    Index Cond: (idents %> 'A partner integration is hammering the invoices list endpoint and getting rate limited, what is the limit and what should they use instead?'::text)
                    Buffers: shared hit=2276
Planning Time: 6.439 ms
Execution Time: 166.167 ms
```

### a_simple_and — exact query

```
Limit (actual time=3217.207..3247.154 rows=10.00 loops=1)
  Buffers: shared hit=64365
  ->  Sort (actual time=3217.205..3247.151 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''idempotency-key'' & ''idempotency'' & ''key'' & ''header'''::tsquery, 0)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=64365
        ->  Gather (actual time=63.609..3246.230 rows=3621.00 loops=1)
              Workers Planned: 2
              Workers Launched: 2
              Buffers: shared hit=64359
              ->  Parallel Append (actual time=63.940..3208.906 rows=1207.00 loops=3)
                    Buffers: shared hit=64359
                    ->  Result (actual time=63.939..3208.634 rows=1207.00 loops=3)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=64359
                          ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m_1 (actual time=63.859..3189.324 rows=1207.00 loops=3)
                                Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''idempotency-key'' & ''idempotency'' & ''key'' & ''header'''::tsquery))
                                Rows Removed by Filter: 215455
                                Heap Blocks: exact=19119
                                Buffers: shared hit=64359
                                Worker 0:  Heap Blocks: exact=19627
                                Worker 1:  Heap Blocks: exact=20057
                                ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=51.742..51.742 rows=649987.00 loops=1)
                                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 1.734 ms
Execution Time: 3247.223 ms
```

### a_simple_and — long query

```
Limit (actual time=3142.857..3173.316 rows=0.00 loops=1)
  Buffers: shared hit=64365
  ->  Sort (actual time=3142.856..3173.314 rows=0.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''a'' & ''partner'' & ''integration'' & ''is'' & ''hammering'' & ''the'' & ''invoices'' & ''list'' & ''endpoint'' & ''and'' & ''getting'' & ''rate'' & ''limited'' & ''what'' & ''is'' & ''the'' & ''limit'' & ''and'' & ''what'' & ''should'' & ''they'' & ''use'' & ''instead'''::tsquery, 0)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=64365
        ->  Gather (actual time=3142.827..3173.285 rows=0.00 loops=1)
              Workers Planned: 2
              Workers Launched: 2
              Buffers: shared hit=64359
              ->  Parallel Append (actual time=3137.642..3137.643 rows=0.00 loops=3)
                    Buffers: shared hit=64359
                    ->  Result (actual time=3137.641..3137.641 rows=0.00 loops=3)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=64359
                          ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m_1 (actual time=3137.635..3137.636 rows=0.00 loops=3)
                                Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''a'' & ''partner'' & ''integration'' & ''is'' & ''hammering'' & ''the'' & ''invoices'' & ''list'' & ''endpoint'' & ''and'' & ''getting'' & ''rate'' & ''limited'' & ''what'' & ''is'' & ''the'' & ''limit'' & ''and'' & ''what'' & ''should'' & ''they'' & ''use'' & ''instead'''::tsquery))
                                Rows Removed by Filter: 216662
                                Heap Blocks: exact=19435
                                Buffers: shared hit=64359
                                Worker 0:  Heap Blocks: exact=19672
                                Worker 1:  Heap Blocks: exact=19696
                                ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=49.528..49.529 rows=649987.00 loops=1)
                                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 1.823 ms
Execution Time: 3173.376 ms
```

### a_simple_and@no_rls — exact query

```
Limit (actual time=106.416..106.419 rows=10.00 loops=1)
  ->  Sort (actual time=106.414..106.415 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''idempotency-key'' & ''idempotency'' & ''key'' & ''header'''::tsquery, 0)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=8.619..105.736 rows=3621.00 loops=1)
              Recheck Cond: (to_tsvector('simple'::regconfig, claim) @@ '''idempotency-key'' & ''idempotency'' & ''key'' & ''header'''::tsquery)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 2239
              Heap Blocks: exact=5584
              ->  Bitmap Index Scan on memories_t_<tenant>_claim_fts (actual time=6.642..6.643 rows=5860.00 loops=1)
                    Index Cond: (to_tsvector('simple'::regconfig, claim) @@ '''idempotency-key'' & ''idempotency'' & ''key'' & ''header'''::tsquery)
                    Buffers: shared hit=68
Planning Time: 5.296 ms
Execution Time: 106.552 ms
```

### a_simple_and@no_rls — long query

```
Limit (actual time=1.033..1.034 rows=0.00 loops=1)
  Buffers: shared hit=112
  ->  Sort (actual time=1.032..1.032 rows=0.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''a'' & ''partner'' & ''integration'' & ''is'' & ''hammering'' & ''the'' & ''invoices'' & ''list'' & ''endpoint'' & ''and'' & ''getting'' & ''rate'' & ''limited'' & ''what'' & ''is'' & ''the'' & ''limit'' & ''and'' & ''what'' & ''should'' & ''they'' & ''use'' & ''instead'''::tsquery, 0)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=112
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=1.004..1.004 rows=0.00 loops=1)
              Recheck Cond: (to_tsvector('simple'::regconfig, claim) @@ '''a'' & ''partner'' & ''integration'' & ''is'' & ''hammering'' & ''the'' & ''invoices'' & ''list'' & ''endpoint'' & ''and'' & ''getting'' & ''rate'' & ''limited'' & ''what'' & ''is'' & ''the'' & ''limit'' & ''and'' & ''what'' & ''should'' & ''they'' & ''use'' & ''instead'''::tsquery)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Buffers: shared hit=106
              ->  Bitmap Index Scan on memories_t_<tenant>_claim_fts (actual time=0.998..0.998 rows=0.00 loops=1)
                    Index Cond: (to_tsvector('simple'::regconfig, claim) @@ '''a'' & ''partner'' & ''integration'' & ''is'' & ''hammering'' & ''the'' & ''invoices'' & ''list'' & ''endpoint'' & ''and'' & ''getting'' & ''rate'' & ''limited'' & ''what'' & ''is'' & ''the'' & ''limit'' & ''and'' & ''what'' & ''should'' & ''they'' & ''use'' & ''instead'''::tsquery)
                    Buffers: shared hit=106
Planning Time: 1.560 ms
Execution Time: 1.065 ms
```

### b_simple_or_rank — exact query

```
Limit (actual time=3549.430..3578.605 rows=10.00 loops=1)
  Buffers: shared hit=64389
  ->  Gather Merge (actual time=3549.429..3578.601 rows=10.00 loops=1)
        Workers Planned: 2
        Workers Launched: 2
        Buffers: shared hit=64389
        ->  Sort (actual time=3544.910..3544.913 rows=7.33 loops=3)
              Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''idempotency-key'' | ''idempotency'' | ''key'' | ''header'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=64389
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              Worker 1:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=50.208..3540.677 rows=16801.00 loops=3)
                    Buffers: shared hit=64359
                    ->  Result (actual time=50.207..3538.886 rows=16801.00 loops=3)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=64359
                          ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m_1 (actual time=50.186..3267.671 rows=16801.00 loops=3)
                                Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''idempotency-key'' | ''idempotency'' | ''key'' | ''header'''::tsquery))
                                Rows Removed by Filter: 199861
                                Heap Blocks: exact=19708
                                Buffers: shared hit=64359
                                Worker 0:  Heap Blocks: exact=19636
                                Worker 1:  Heap Blocks: exact=19459
                                ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=45.101..45.102 rows=649987.00 loops=1)
                                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 1.697 ms
Execution Time: 3578.680 ms
```

### b_simple_or_rank — long query

```
Limit (actual time=6571.011..6602.178 rows=10.00 loops=1)
  Buffers: shared hit=64389
  ->  Gather Merge (actual time=6571.009..6602.174 rows=10.00 loops=1)
        Workers Planned: 2
        Workers Launched: 2
        Buffers: shared hit=64389
        ->  Sort (actual time=6566.381..6566.383 rows=8.33 loops=3)
              Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''a'' | ''partner'' | ''integration'' | ''is'' | ''hammering'' | ''the'' | ''invoices'' | ''list'' | ''endpoint'' | ''and'' | ''getting'' | ''rate'' | ''limited'' | ''what'' | ''is'' | ''the'' | ''limit'' | ''and'' | ''what'' | ''should'' | ''they'' | ''use'' | ''instead'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=64389
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              Worker 1:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=52.013..6538.519 rows=197690.67 loops=3)
                    Buffers: shared hit=64359
                    ->  Result (actual time=52.012..6520.010 rows=197690.67 loops=3)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=64359
                          ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m_1 (actual time=51.968..3390.552 rows=197690.67 loops=3)
                                Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''a'' | ''partner'' | ''integration'' | ''is'' | ''hammering'' | ''the'' | ''invoices'' | ''list'' | ''endpoint'' | ''and'' | ''getting'' | ''rate'' | ''limited'' | ''what'' | ''is'' | ''the'' | ''limit'' | ''and'' | ''what'' | ''should'' | ''they'' | ''use'' | ''instead'''::tsquery))
                                Rows Removed by Filter: 18972
                                Heap Blocks: exact=19652
                                Buffers: shared hit=64359
                                Worker 0:  Heap Blocks: exact=19235
                                Worker 1:  Heap Blocks: exact=19916
                                ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=45.329..45.329 rows=649987.00 loops=1)
                                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 1.719 ms
Execution Time: 6602.253 ms
```

### b_simple_or_rank@no_rls — exact query

```
Limit (actual time=470.636..498.977 rows=10.00 loops=1)
  Buffers: shared hit=41662
  ->  Gather Merge (actual time=470.635..498.973 rows=10.00 loops=1)
        Workers Planned: 2
        Workers Launched: 2
        Buffers: shared hit=41662
        ->  Sort (actual time=465.204..465.207 rows=8.00 loops=3)
              Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''idempotency-key'' | ''idempotency'' | ''key'' | ''header'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=41662
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              Worker 1:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m (actual time=88.254..462.942 rows=16801.00 loops=3)
                    Recheck Cond: ((to_tsvector('simple'::regconfig, claim) @@ '''idempotency-key'' | ''idempotency'' | ''key'' | ''header'''::tsquery) AND (tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])))
                    Rows Removed by Filter: 1264
                    Heap Blocks: exact=12041
                    Buffers: shared hit=41632
                    Worker 0:  Heap Blocks: exact=11959
                    Worker 1:  Heap Blocks: exact=12025
                    ->  BitmapAnd (actual time=86.419..86.421 rows=0.00 loops=1)
                          ->  Bitmap Index Scan on memories_t_<tenant>_claim_fts (actual time=20.850..20.851 rows=83409.00 loops=1)
                                Index Cond: (to_tsvector('simple'::regconfig, claim) @@ '''idempotency-key'' | ''idempotency'' | ''key'' | ''header'''::tsquery)
                                Buffers: shared hit=45
                          ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=63.660..63.660 rows=649987.00 loops=1)
                                Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 2.328 ms
Execution Time: 499.080 ms
```

### b_simple_or_rank@no_rls — long query

```
Limit (actual time=6494.422..6526.236 rows=10.00 loops=1)
  Buffers: shared hit=64395
  ->  Gather Merge (actual time=6494.421..6526.233 rows=10.00 loops=1)
        Workers Planned: 2
        Workers Launched: 2
        Buffers: shared hit=64395
        ->  Sort (actual time=6490.140..6490.142 rows=8.00 loops=3)
              Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''a'' | ''partner'' | ''integration'' | ''is'' | ''hammering'' | ''the'' | ''invoices'' | ''list'' | ''endpoint'' | ''and'' | ''getting'' | ''rate'' | ''limited'' | ''what'' | ''is'' | ''the'' | ''limit'' | ''and'' | ''what'' | ''should'' | ''they'' | ''use'' | ''instead'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=64395
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              Worker 1:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m (actual time=47.215..6463.142 rows=197690.67 loops=3)
                    Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''a'' | ''partner'' | ''integration'' | ''is'' | ''hammering'' | ''the'' | ''invoices'' | ''list'' | ''endpoint'' | ''and'' | ''getting'' | ''rate'' | ''limited'' | ''what'' | ''is'' | ''the'' | ''limit'' | ''and'' | ''what'' | ''should'' | ''they'' | ''use'' | ''instead'''::tsquery))
                    Rows Removed by Filter: 18972
                    Heap Blocks: exact=19794
                    Buffers: shared hit=64365
                    Worker 0:  Heap Blocks: exact=19335
                    Worker 1:  Heap Blocks: exact=19674
                    ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=42.188..42.188 rows=649987.00 loops=1)
                          Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 1.753 ms
Execution Time: 6526.289 ms
```

### b_simple_or_rank_cd — exact query

```
Limit (actual time=3576.534..3608.759 rows=10.00 loops=1)
  Buffers: shared hit=64389
  ->  Gather Merge (actual time=3576.532..3608.755 rows=10.00 loops=1)
        Workers Planned: 2
        Workers Launched: 2
        Buffers: shared hit=64389
        ->  Sort (actual time=3566.361..3566.363 rows=7.67 loops=3)
              Sort Key: (ts_rank_cd(to_tsvector('simple'::regconfig, m.claim), '''idempotency-key'' | ''idempotency'' | ''key'' | ''header'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=64389
              Worker 0:  Sort Method: top-N heapsort  Memory: 25kB
              Worker 1:  Sort Method: top-N heapsort  Memory: 25kB
              ->  Parallel Append (actual time=65.901..3562.578 rows=16801.00 loops=3)
                    Buffers: shared hit=64359
                    ->  Result (actual time=65.900..3560.685 rows=16801.00 loops=3)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=64359
                          ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m_1 (actual time=65.631..3247.473 rows=16801.00 loops=3)
                                Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''idempotency-key'' | ''idempotency'' | ''key'' | ''header'''::tsquery))
                                Rows Removed by Filter: 199861
                                Heap Blocks: exact=19427
                                Buffers: shared hit=64359
                                Worker 0:  Heap Blocks: exact=19726
                                Worker 1:  Heap Blocks: exact=19650
                                ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=63.235..63.235 rows=649987.00 loops=1)
                                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 3.361 ms
Execution Time: 3608.938 ms
```

### b_simple_or_rank_cd — long query

```
Limit (actual time=10202.984..10237.889 rows=10.00 loops=1)
  Buffers: shared hit=64389
  ->  Gather Merge (actual time=10202.983..10237.885 rows=10.00 loops=1)
        Workers Planned: 2
        Workers Launched: 2
        Buffers: shared hit=64389
        ->  Sort (actual time=10198.227..10198.229 rows=7.67 loops=3)
              Sort Key: (ts_rank_cd(to_tsvector('simple'::regconfig, m.claim), '''a'' | ''partner'' | ''integration'' | ''is'' | ''hammering'' | ''the'' | ''invoices'' | ''list'' | ''endpoint'' | ''and'' | ''getting'' | ''rate'' | ''limited'' | ''what'' | ''is'' | ''the'' | ''limit'' | ''and'' | ''what'' | ''should'' | ''they'' | ''use'' | ''instead'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=64389
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              Worker 1:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=49.789..10155.277 rows=197690.67 loops=3)
                    Buffers: shared hit=64359
                    ->  Result (actual time=49.788..10127.815 rows=197690.67 loops=3)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=64359
                          ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m_1 (actual time=48.847..3450.912 rows=197690.67 loops=3)
                                Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''a'' | ''partner'' | ''integration'' | ''is'' | ''hammering'' | ''the'' | ''invoices'' | ''list'' | ''endpoint'' | ''and'' | ''getting'' | ''rate'' | ''limited'' | ''what'' | ''is'' | ''the'' | ''limit'' | ''and'' | ''what'' | ''should'' | ''they'' | ''use'' | ''instead'''::tsquery))
                                Rows Removed by Filter: 18972
                                Heap Blocks: exact=19356
                                Buffers: shared hit=64359
                                Worker 0:  Heap Blocks: exact=19882
                                Worker 1:  Heap Blocks: exact=19565
                                ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=44.051..44.051 rows=649987.00 loops=1)
                                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 1.615 ms
Execution Time: 10237.965 ms
```

### d_bm25_querytime — exact query

```
Limit (actual time=4217.733..4220.891 rows=10.00 loops=1)
  Buffers: shared hit=650847
  CTE qt
    ->  Aggregate (actual time=0.085..0.086 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Sort (actual time=0.074..0.076 rows=4.00 loops=1)
                Sort Key: x.x
                Sort Method: quicksort  Memory: 25kB
                Buffers: shared hit=3
                ->  Function Scan on unnest x (actual time=0.017..0.018 rows=4.00 loops=1)
  CTE st
    ->  Finalize Aggregate (actual time=475.958..476.098 rows=1.00 loops=1)
          ->  Gather (actual time=475.928..476.080 rows=2.00 loops=1)
                Workers Planned: 1
                Workers Launched: 1
                ->  Partial Aggregate (actual time=470.402..470.403 rows=1.00 loops=2)
                      ->  Parallel Append (actual time=0.201..452.703 rows=302384.50 loops=2)
                            ->  Result (actual time=0.200..432.380 rows=302384.50 loops=2)
                                  One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                                  ->  Parallel Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m_1 (actual time=0.189..410.177 rows=302384.50 loops=2)
                                        Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                        Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])))
                                        Rows Removed by Filter: 22609
  CTE tf
    ->  Result (actual time=86.051..3636.613 rows=69719.00 loops=1)
          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
          Buffers: shared hit=64362
          ->  Nested Loop (actual time=86.014..3627.323 rows=69719.00 loops=1)
                Join Filter: (x_1.lexeme = ANY (qt.terms))
                Rows Removed by Join Filter: 1533325
                Buffers: shared hit=64362
                ->  Nested Loop (actual time=85.923..2209.610 rows=50403.00 loops=1)
                      Buffers: shared hit=64362
                      ->  CTE Scan on qt (actual time=0.086..0.087 rows=1.00 loops=1)
                            Storage: Memory  Maximum Storage: 17kB
                            Buffers: shared hit=3
                      ->  Gather (actual time=85.834..2203.187 rows=50403.00 loops=1)
                            Workers Planned: 2
                            Workers Launched: 2
                            Buffers: shared hit=64359
```

### d_bm25_querytime — long query

```
(timed out)
```

### d_bm25_maintained — exact query

```
Limit (actual time=3781.406..3798.105 rows=10.00 loops=1)
  Buffers: shared hit=66200
  CTE qt
    ->  Aggregate (actual time=0.081..0.084 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Sort (actual time=0.062..0.065 rows=4.00 loops=1)
                Sort Key: x.x
                Sort Method: quicksort  Memory: 25kB
                Buffers: shared hit=3
                ->  Function Scan on unnest x (actual time=0.019..0.020 rows=4.00 loops=1)
  CTE st
    ->  Aggregate (actual time=0.157..0.159 rows=1.00 loops=1)
          ->  Result (actual time=0.033..0.120 rows=451.00 loops=1)
                One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                ->  Seq Scan on bm25_scope s (actual time=0.026..0.076 rows=451.00 loops=1)
                      Filter: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
  CTE df
    ->  GroupAggregate (actual time=3.017..3.163 rows=4.00 loops=1)
          Group Key: d.term
          Buffers: shared hit=1806
          ->  Sort (actual time=2.966..3.030 rows=1778.00 loops=1)
                Sort Key: d.term
                Sort Method: quicksort  Memory: 104kB
                Buffers: shared hit=1806
                ->  Result (actual time=0.119..2.737 rows=1778.00 loops=1)
                      One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                      Buffers: shared hit=1806
                      ->  Nested Loop (actual time=0.117..2.545 rows=1778.00 loops=1)
                            Buffers: shared hit=1806
                            ->  CTE Scan on qt (actual time=0.001..0.002 rows=1.00 loops=1)
                                  Storage: Memory  Maximum Storage: 17kB
                            ->  Index Scan using bm25_df_pkey on bm25_df d (actual time=0.113..2.364 rows=1778.00 loops=1)
                                  Index Cond: ((tenant_id = '<uuid>'::uuid) AND (term = ANY (qt.terms)) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                  Buffers: shared hit=1806
  CTE tf
    ->  Result (actual time=68.856..3671.869 rows=69746.00 loops=1)
          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
          Buffers: shared hit=64383
          ->  Nested Loop (actual time=68.848..3661.806 rows=69746.00 loops=1)
                Join Filter: (x_1.lexeme = ANY (qt_1.terms))
```

### d_bm25_maintained — long query

```
(timed out)
```

## 1M vector run


### f_hnsw — exact query

```
Limit (actual time=2.675..2.708 rows=10.00 loops=1)
  Buffers: shared hit=1224
  ->  Result (actual time=2.674..2.705 rows=10.00 loops=1)
        One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
        Buffers: shared hit=1224
        ->  Index Scan using memory_vectors_t_<tenant>_hnsw on memory_vectors_t_<tenant> v (actual time=2.666..2.694 rows=10.00 loops=1)
              Order By: (embedding <=> '[…384 floats…]'::halfvec)
              Filter: (served AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 5
              Buffers: shared hit=1224
  Buffers: shared hit=387
Planning Time: 1.904 ms
Execution Time: 2.765 ms
```

### f_hnsw — long query

```
Limit (actual time=12.859..13.040 rows=10.00 loops=1)
  Buffers: shared hit=1152
  ->  Result (actual time=12.858..13.035 rows=10.00 loops=1)
        One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
        Buffers: shared hit=1152
        ->  Index Scan using memory_vectors_t_<tenant>_hnsw on memory_vectors_t_<tenant> v (actual time=12.850..13.022 rows=10.00 loops=1)
              Order By: (embedding <=> '[…384 floats…]'::halfvec)
              Filter: (served AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 5
              Buffers: shared hit=1152
  Buffers: shared hit=387
Planning Time: 2.688 ms
Execution Time: 13.159 ms
```

## 10 × 100k


### a_simple_and — exact query

```
Limit (actual time=338.086..345.093 rows=10.00 loops=1)
  Buffers: shared hit=6634
  ->  Sort (actual time=338.084..345.090 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''pgbouncer'' & ''transaction'' & ''mode'''::tsquery, 0)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=6634
        ->  Gather (actual time=8.773..344.959 rows=242.00 loops=1)
              Workers Planned: 2
              Workers Launched: 2
              Buffers: shared hit=6628
              ->  Parallel Append (actual time=6.485..334.203 rows=80.67 loops=3)
                    Buffers: shared hit=6628
                    ->  Result (actual time=6.484..334.181 rows=80.67 loops=3)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=6628
                          ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m_1 (actual time=6.428..332.748 rows=80.67 loops=3)
                                Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' & ''transaction'' & ''mode'''::tsquery))
                                Rows Removed by Filter: 21550
                                Heap Blocks: exact=1906
                                Buffers: shared hit=6628
                                Worker 0:  Heap Blocks: exact=1883
                                Worker 1:  Heap Blocks: exact=2079
                                ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=5.900..5.900 rows=64892.00 loops=1)
                                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 1.967 ms
Execution Time: 345.166 ms
```

### a_simple_and — long query

```
Limit (actual time=350.578..357.592 rows=0.00 loops=1)
  Buffers: shared hit=6634
  ->  Sort (actual time=350.577..357.590 rows=0.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery, 0)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=6634
        ->  Gather (actual time=350.550..357.563 rows=0.00 loops=1)
              Workers Planned: 2
              Workers Launched: 2
              Buffers: shared hit=6628
              ->  Parallel Append (actual time=346.984..346.985 rows=0.00 loops=3)
                    Buffers: shared hit=6628
                    ->  Result (actual time=346.982..346.983 rows=0.00 loops=3)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=6628
                          ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m_1 (actual time=346.977..346.978 rows=0.00 loops=3)
                                Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery))
                                Rows Removed by Filter: 21631
                                Heap Blocks: exact=1876
                                Buffers: shared hit=6628
                                Worker 0:  Heap Blocks: exact=1769
                                Worker 1:  Heap Blocks: exact=2223
                                ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=4.792..4.793 rows=64892.00 loops=1)
                                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 1.281 ms
Execution Time: 357.636 ms
```

### a_simple_and@no_rls — exact query

```
Limit (actual time=8.316..8.319 rows=10.00 loops=1)
  Buffers: shared hit=407
  ->  Sort (actual time=8.315..8.316 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''pgbouncer'' & ''transaction'' & ''mode'''::tsquery, 0)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=407
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=0.390..8.221 rows=242.00 loops=1)
              Recheck Cond: (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' & ''transaction'' & ''mode'''::tsquery)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 159
              Heap Blocks: exact=389
              Buffers: shared hit=401
              ->  Bitmap Index Scan on memories_t_<tenant>_claim_fts (actual time=0.261..0.261 rows=401.00 loops=1)
                    Index Cond: (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' & ''transaction'' & ''mode'''::tsquery)
                    Buffers: shared hit=12
Planning Time: 1.645 ms
Execution Time: 8.353 ms
```

### a_simple_and@no_rls — long query

```
Limit (actual time=1.686..1.687 rows=0.00 loops=1)
  Buffers: shared hit=100
  ->  Sort (actual time=1.684..1.685 rows=0.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery, 0)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=100
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=1.624..1.625 rows=0.00 loops=1)
              Recheck Cond: (to_tsvector('simple'::regconfig, claim) @@ '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Buffers: shared hit=94
              ->  Bitmap Index Scan on memories_t_<tenant>_claim_fts (actual time=1.611..1.612 rows=0.00 loops=1)
                    Index Cond: (to_tsvector('simple'::regconfig, claim) @@ '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery)
                    Buffers: shared hit=94
Planning Time: 3.158 ms
Execution Time: 1.742 ms
```

### b_simple_or_rank — exact query

```
Limit (actual time=344.615..350.965 rows=10.00 loops=1)
  Buffers: shared hit=6658
  ->  Gather Merge (actual time=344.614..350.962 rows=10.00 loops=1)
        Workers Planned: 2
        Workers Launched: 2
        Buffers: shared hit=6658
        ->  Sort (actual time=340.103..340.105 rows=8.67 loops=3)
              Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=6658
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              Worker 1:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=2.446..339.828 rows=553.67 loops=3)
                    Buffers: shared hit=6628
                    ->  Result (actual time=2.446..339.753 rows=553.67 loops=3)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=6628
                          ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m_1 (actual time=2.408..330.339 rows=553.67 loops=3)
                                Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery))
                                Rows Removed by Filter: 21077
                                Heap Blocks: exact=1967
                                Buffers: shared hit=6628
                                Worker 0:  Heap Blocks: exact=1831
                                Worker 1:  Heap Blocks: exact=2070
                                ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=3.887..3.887 rows=64892.00 loops=1)
                                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 1.356 ms
Execution Time: 351.023 ms
```

### b_simple_or_rank — long query

```
Limit (actual time=667.134..674.174 rows=10.00 loops=1)
  Buffers: shared hit=6658
  ->  Gather Merge (actual time=667.133..674.170 rows=10.00 loops=1)
        Workers Planned: 2
        Workers Launched: 2
        Buffers: shared hit=6658
        ->  Sort (actual time=663.451..663.453 rows=7.67 loops=3)
              Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=6658
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              Worker 1:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=1.868..660.512 rows=19538.67 loops=3)
                    Buffers: shared hit=6628
                    ->  Result (actual time=1.868..658.676 rows=19538.67 loops=3)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=6628
                          ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m_1 (actual time=1.828..336.777 rows=19538.67 loops=3)
                                Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery))
                                Rows Removed by Filter: 2092
                                Heap Blocks: exact=1893
                                Buffers: shared hit=6628
                                Worker 0:  Heap Blocks: exact=2063
                                Worker 1:  Heap Blocks: exact=1912
                                ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=4.095..4.095 rows=64892.00 loops=1)
                                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 1.476 ms
Execution Time: 674.245 ms
```

### b_simple_or_rank@no_rls — exact query

```
Limit (actual time=50.466..50.470 rows=10.00 loops=1)
  Buffers: shared hit=2125
  ->  Sort (actual time=50.464..50.466 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery, 0)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=2125
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=6.194..50.103 rows=1661.00 loops=1)
              Recheck Cond: ((to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery) AND (tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
              Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])))
              Rows Removed by Filter: 142
              Heap Blocks: exact=1573
              Buffers: shared hit=2119
              ->  BitmapAnd (actual time=5.937..5.938 rows=0.00 loops=1)
                    ->  Bitmap Index Scan on memories_t_<tenant>_claim_fts (actual time=0.790..0.791 rows=2772.00 loops=1)
                          Index Cond: (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery)
                          Buffers: shared hit=12
                    ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=4.993..4.993 rows=64892.00 loops=1)
                          Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 1.845 ms
Execution Time: 50.587 ms
```

### b_simple_or_rank@no_rls — long query

```
Limit (actual time=672.527..679.985 rows=10.00 loops=1)
  Buffers: shared hit=6664
  ->  Gather Merge (actual time=672.525..679.981 rows=10.00 loops=1)
        Workers Planned: 2
        Workers Launched: 2
        Buffers: shared hit=6664
        ->  Sort (actual time=667.776..667.778 rows=8.67 loops=3)
              Sort Key: (ts_rank(to_tsvector('simple'::regconfig, m.claim), '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=6664
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              Worker 1:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m (actual time=2.111..664.783 rows=19538.67 loops=3)
                    Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery))
                    Rows Removed by Filter: 2092
                    Heap Blocks: exact=2037
                    Buffers: shared hit=6634
                    Worker 0:  Heap Blocks: exact=1906
                    Worker 1:  Heap Blocks: exact=1925
                    ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=4.641..4.641 rows=64892.00 loops=1)
                          Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 1.829 ms
Execution Time: 680.053 ms
```

### b_simple_or_rank_cd — exact query

```
Limit (actual time=335.470..342.352 rows=10.00 loops=1)
  Buffers: shared hit=6658
  ->  Gather Merge (actual time=335.469..342.349 rows=10.00 loops=1)
        Workers Planned: 2
        Workers Launched: 2
        Buffers: shared hit=6658
        ->  Sort (actual time=329.613..329.617 rows=8.00 loops=3)
              Sort Key: (ts_rank_cd(to_tsvector('simple'::regconfig, m.claim), '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=6658
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              Worker 1:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=3.176..329.355 rows=553.67 loops=3)
                    Buffers: shared hit=6628
                    ->  Result (actual time=3.175..329.269 rows=553.67 loops=3)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=6628
                          ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m_1 (actual time=2.956..318.946 rows=553.67 loops=3)
                                Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery))
                                Rows Removed by Filter: 21077
                                Heap Blocks: exact=2100
                                Buffers: shared hit=6628
                                Worker 0:  Heap Blocks: exact=1878
                                Worker 1:  Heap Blocks: exact=1890
                                ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=3.753..3.754 rows=64892.00 loops=1)
                                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 1.256 ms
Execution Time: 342.405 ms
```

### b_simple_or_rank_cd — long query

```
Limit (actual time=979.581..986.757 rows=10.00 loops=1)
  Buffers: shared hit=6658
  ->  Gather Merge (actual time=979.579..986.753 rows=10.00 loops=1)
        Workers Planned: 2
        Workers Launched: 2
        Buffers: shared hit=6658
        ->  Sort (actual time=976.204..976.206 rows=8.33 loops=3)
              Sort Key: (ts_rank_cd(to_tsvector('simple'::regconfig, m.claim), '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0)) DESC, m.id
              Sort Method: top-N heapsort  Memory: 26kB
              Buffers: shared hit=6658
              Worker 0:  Sort Method: top-N heapsort  Memory: 26kB
              Worker 1:  Sort Method: top-N heapsort  Memory: 26kB
              ->  Parallel Append (actual time=2.721..972.124 rows=19538.67 loops=3)
                    Buffers: shared hit=6628
                    ->  Result (actual time=2.720..969.298 rows=19538.67 loops=3)
                          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                          Buffers: shared hit=6628
                          ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m_1 (actual time=1.930..332.541 rows=19538.67 loops=3)
                                Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                                Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery))
                                Rows Removed by Filter: 2092
                                Heap Blocks: exact=1954
                                Buffers: shared hit=6628
                                Worker 0:  Heap Blocks: exact=1947
                                Worker 1:  Heap Blocks: exact=1967
                                ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=3.822..3.822 rows=64892.00 loops=1)
                                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
Planning Time: 1.284 ms
Execution Time: 986.817 ms
```

### d_bm25_querytime — exact query

```
Limit (actual time=365.978..368.666 rows=10.00 loops=1)
  CTE qt
    ->  Aggregate (actual time=0.074..0.076 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Sort (actual time=0.065..0.067 rows=3.00 loops=1)
                Sort Key: x.x
                Sort Method: quicksort  Memory: 25kB
                Buffers: shared hit=3
                ->  Function Scan on unnest x (actual time=0.027..0.028 rows=3.00 loops=1)
  CTE st
    ->  Aggregate (actual time=52.409..52.410 rows=1.00 loops=1)
          Buffers: shared hit=43687
          ->  Result (actual time=0.054..48.792 rows=60440.00 loops=1)
                One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                Buffers: shared hit=43687
                ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.047..44.156 rows=60440.00 loops=1)
                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                      Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])))
                      Rows Removed by Filter: 4452
                      Buffers: shared hit=43687
  CTE tf
    ->  Result (actual time=20.980..312.306 rows=2645.00 loops=1)
          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
          Buffers: shared hit=6631
          ->  Nested Loop (actual time=20.972..311.933 rows=2645.00 loops=1)
                Join Filter: (x_1.lexeme = ANY (qt.terms))
                Rows Removed by Join Filter: 48664
                Buffers: shared hit=6631
                ->  Nested Loop (actual time=20.900..268.195 rows=1661.00 loops=1)
                      Buffers: shared hit=6631
                      ->  CTE Scan on qt (actual time=0.075..0.076 rows=1.00 loops=1)
                            Storage: Memory  Maximum Storage: 17kB
                            Buffers: shared hit=3
                      ->  Gather (actual time=20.821..267.900 rows=1661.00 loops=1)
                            Workers Planned: 2
                            Workers Launched: 2
                            Buffers: shared hit=6628
                            ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m_1 (actual time=9.321..322.652 rows=553.67 loops=3)
                                  Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                                  Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery))
```

### d_bm25_querytime — long query

```
Limit (actual time=1976.485..1976.851 rows=10.00 loops=1)
  CTE qt
    ->  Aggregate (actual time=0.049..0.051 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Sort (actual time=0.041..0.043 rows=22.00 loops=1)
                Sort Key: x.x
                Sort Method: quicksort  Memory: 25kB
                Buffers: shared hit=3
                ->  Function Scan on unnest x (actual time=0.010..0.012 rows=22.00 loops=1)
  CTE st
    ->  Aggregate (actual time=47.977..47.979 rows=1.00 loops=1)
          Buffers: shared hit=43687
          ->  Result (actual time=0.026..44.708 rows=60440.00 loops=1)
                One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                Buffers: shared hit=43687
                ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.023..40.510 rows=60440.00 loops=1)
                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                      Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])))
                      Rows Removed by Filter: 4452
                      Buffers: shared hit=43687
  CTE tf
    ->  Result (actual time=5.547..1700.202 rows=165921.00 loops=1)
          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
          Buffers: shared hit=6631
          ->  Nested Loop (actual time=5.543..1679.942 rows=165921.00 loops=1)
                Join Filter: (x_1.lexeme = ANY (qt.terms))
                Rows Removed by Join Filter: 1433953
                Buffers: shared hit=6631
                ->  Nested Loop (actual time=5.504..22.260 rows=58616.00 loops=1)
                      Buffers: shared hit=6631
                      ->  CTE Scan on qt (actual time=0.049..0.050 rows=1.00 loops=1)
                            Storage: Memory  Maximum Storage: 17kB
                            Buffers: shared hit=3
                      ->  Gather (actual time=5.453..15.629 rows=58616.00 loops=1)
                            Workers Planned: 2
                            Workers Launched: 2
                            Buffers: shared hit=6628
                            ->  Parallel Bitmap Heap Scan on memories_t_<tenant> m_1 (actual time=1.991..349.041 rows=19538.67 loops=3)
                                  Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
                                  Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery))
```

### a_simple_and@flat — exact query

```
Limit (actual time=918.119..918.123 rows=10.00 loops=1)
  Buffers: shared hit=44182
  ->  Sort (actual time=918.117..918.119 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, claim), '''pgbouncer'' & ''transaction'' & ''mode'''::tsquery, 0)) DESC, id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=44182
        ->  Result (actual time=7.460..917.951 rows=245.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              Buffers: shared hit=44176
              ->  Index Scan using memories_flat_served_tip on memories_flat m (actual time=7.426..914.049 rows=245.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' & ''transaction'' & ''mode'''::tsquery))
                    Rows Removed by Filter: 64947
                    Buffers: shared hit=44176
  Buffers: shared hit=305
Planning Time: 1.034 ms
Execution Time: 918.169 ms
```

### a_simple_and@flat — long query

```
Limit (actual time=1007.322..1007.324 rows=0.00 loops=1)
  Buffers: shared hit=44182
  ->  Sort (actual time=1007.320..1007.321 rows=0.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, claim), '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery, 0)) DESC, id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=44182
        ->  Result (actual time=1007.286..1007.287 rows=0.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              Buffers: shared hit=44176
              ->  Index Scan using memories_flat_served_tip on memories_flat m (actual time=1007.279..1007.280 rows=0.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery))
                    Rows Removed by Filter: 65192
                    Buffers: shared hit=44176
  Buffers: shared hit=305
Planning Time: 1.941 ms
Execution Time: 1007.388 ms
```

### a_simple_and@flat@no_rls — exact query

```
Limit (actual time=28.622..28.629 rows=10.00 loops=1)
  Buffers: shared hit=3905
  ->  Sort (actual time=28.620..28.624 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, claim), '''pgbouncer'' & ''transaction'' & ''mode'''::tsquery, 0)) DESC, id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=3905
        ->  Bitmap Heap Scan on memories_flat m (actual time=5.975..28.465 rows=245.00 loops=1)
              Recheck Cond: (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' & ''transaction'' & ''mode'''::tsquery)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 3743
              Heap Blocks: exact=3865
              Buffers: shared hit=3899
              ->  Bitmap Index Scan on memories_flat_fts (actual time=4.685..4.685 rows=3988.00 loops=1)
                    Index Cond: (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' & ''transaction'' & ''mode'''::tsquery)
                    Buffers: shared hit=34
  Buffers: shared hit=300
Planning Time: 1.647 ms
Execution Time: 28.776 ms
```

### a_simple_and@flat@no_rls — long query

```
Limit (actual time=1.019..1.020 rows=0.00 loops=1)
  Buffers: shared hit=109
  ->  Sort (actual time=1.018..1.018 rows=0.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, claim), '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery, 0)) DESC, id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=109
        ->  Bitmap Heap Scan on memories_flat m (actual time=0.999..0.999 rows=0.00 loops=1)
              Recheck Cond: (to_tsvector('simple'::regconfig, claim) @@ '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Buffers: shared hit=103
              ->  Bitmap Index Scan on memories_flat_fts (actual time=0.992..0.992 rows=0.00 loops=1)
                    Index Cond: (to_tsvector('simple'::regconfig, claim) @@ '''the'' & ''worker'' & ''is'' & ''killing'' & ''pdf'' & ''render'' & ''jobs'' & ''during'' & ''a'' & ''rolling'' & ''deploy'' & ''and'' & ''invoices'' & ''end'' & ''up'' & ''half'' & ''rendered'' & ''how'' & ''do'' & ''we'' & ''stop'' & ''that'''::tsquery)
                    Buffers: shared hit=103
  Buffers: shared hit=300
Planning Time: 0.858 ms
Execution Time: 1.047 ms
```

### b_simple_or_rank@flat — exact query

```
Limit (actual time=1024.295..1024.298 rows=10.00 loops=1)
  Buffers: shared hit=44182
  ->  Sort (actual time=1024.294..1024.295 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, claim), '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery, 0)) DESC, id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=44182
        ->  Result (actual time=0.801..1023.659 rows=1671.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              Buffers: shared hit=44176
              ->  Index Scan using memories_flat_served_tip on memories_flat m (actual time=0.783..995.527 rows=1671.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery))
                    Rows Removed by Filter: 63521
                    Buffers: shared hit=44176
  Buffers: shared hit=317
Planning Time: 0.880 ms
Execution Time: 1024.368 ms
```

### b_simple_or_rank@flat — long query

```
Limit (actual time=1926.175..1926.178 rows=10.00 loops=1)
  Buffers: shared hit=44182
  ->  Sort (actual time=1926.174..1926.175 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, claim), '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0)) DESC, id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=44182
        ->  Result (actual time=0.122..1918.134 rows=58907.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              Buffers: shared hit=44176
              ->  Index Scan using memories_flat_served_tip on memories_flat m (actual time=0.096..989.911 rows=58907.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery))
                    Rows Removed by Filter: 6285
                    Buffers: shared hit=44176
  Buffers: shared hit=317
Planning Time: 0.886 ms
Execution Time: 1926.219 ms
```

### b_simple_or_rank@flat@no_rls — exact query

```
Limit (actual time=59.661..59.665 rows=10.00 loops=1)
  Buffers: shared hit=2140
  ->  Sort (actual time=59.659..59.662 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, claim), '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery, 0)) DESC, id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=2140
        ->  Bitmap Heap Scan on memories_flat m (actual time=15.043..59.173 rows=1671.00 loops=1)
              Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text) AND (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery))
              Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])))
              Rows Removed by Filter: 142
              Heap Blocks: exact=1570
              Buffers: shared hit=2134
              ->  BitmapAnd (actual time=14.804..14.805 rows=0.00 loops=1)
                    ->  Bitmap Index Scan on memories_flat_served_tip (actual time=7.267..7.267 rows=65192.00 loops=1)
                          Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    ->  Bitmap Index Scan on memories_flat_fts (actual time=6.998..6.999 rows=27290.00 loops=1)
                          Index Cond: (to_tsvector('simple'::regconfig, claim) @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery)
                          Buffers: shared hit=26
  Buffers: shared hit=315
Planning Time: 1.354 ms
Execution Time: 59.790 ms
```

### b_simple_or_rank@flat@no_rls — long query

```
Limit (actual time=2013.989..2013.992 rows=10.00 loops=1)
  Buffers: shared hit=44182
  ->  Sort (actual time=2013.988..2013.989 rows=10.00 loops=1)
        Sort Key: (ts_rank(to_tsvector('simple'::regconfig, claim), '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0)) DESC, id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=44182
        ->  Index Scan using memories_flat_served_tip on memories_flat m (actual time=0.083..2005.258 rows=58907.00 loops=1)
              Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND (to_tsvector('simple'::regconfig, claim) @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery))
              Rows Removed by Filter: 6285
              Buffers: shared hit=44176
  Buffers: shared hit=315
Planning Time: 0.859 ms
Execution Time: 2014.007 ms
```

### d_bm25_maintained — exact query

```
Limit (actual time=364.341..365.543 rows=10.00 loops=1)
  Buffers: shared hit=6806
  CTE qt
    ->  Aggregate (actual time=0.083..0.085 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Sort (actual time=0.075..0.077 rows=3.00 loops=1)
                Sort Key: x.x
                Sort Method: quicksort  Memory: 25kB
                Buffers: shared hit=3
                ->  Function Scan on unnest x (actual time=0.010..0.011 rows=3.00 loops=1)
  CTE st
    ->  Aggregate (actual time=0.152..0.153 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Result (actual time=0.110..0.130 rows=46.00 loops=1)
                One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                Buffers: shared hit=3
                ->  Index Scan using bm25_scope_pkey on bm25_scope s (actual time=0.077..0.087 rows=46.00 loops=1)
                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                      Buffers: shared hit=3
  CTE df
    ->  GroupAggregate (actual time=1.035..1.063 rows=3.00 loops=1)
          Group Key: d.term
          Buffers: shared hit=144
          ->  Sort (actual time=1.016..1.030 rows=137.00 loops=1)
                Sort Key: d.term
                Sort Method: quicksort  Memory: 29kB
                Buffers: shared hit=144
                ->  Result (actual time=0.084..0.955 rows=137.00 loops=1)
                      One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                      Buffers: shared hit=144
                      ->  Nested Loop (actual time=0.081..0.900 rows=137.00 loops=1)
                            Buffers: shared hit=144
                            ->  CTE Scan on qt (actual time=0.001..0.002 rows=1.00 loops=1)
                                  Storage: Memory  Maximum Storage: 17kB
                            ->  Index Scan using bm25_df_pkey on bm25_df d (actual time=0.076..0.859 rows=137.00 loops=1)
                                  Index Cond: ((tenant_id = '<uuid>'::uuid) AND (term = ANY (qt.terms)) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                  Buffers: shared hit=144
  CTE tf
    ->  Result (actual time=16.482..358.976 rows=2662.00 loops=1)
          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
```

### d_bm25_maintained — long query

```
Limit (actual time=1871.918..1872.219 rows=10.00 loops=1)
  Buffers: shared hit=7552
  CTE qt
    ->  Aggregate (actual time=0.044..0.046 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Sort (actual time=0.037..0.039 rows=22.00 loops=1)
                Sort Key: x.x
                Sort Method: quicksort  Memory: 25kB
                Buffers: shared hit=3
                ->  Function Scan on unnest x (actual time=0.008..0.009 rows=22.00 loops=1)
  CTE st
    ->  Aggregate (actual time=0.053..0.054 rows=1.00 loops=1)
          Buffers: shared hit=3
          ->  Result (actual time=0.035..0.044 rows=46.00 loops=1)
                One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                Buffers: shared hit=3
                ->  Index Scan using bm25_scope_pkey on bm25_scope s (actual time=0.032..0.036 rows=46.00 loops=1)
                      Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                      Buffers: shared hit=3
  CTE df
    ->  GroupAggregate (actual time=1.970..2.059 rows=18.00 loops=1)
          Group Key: d.term
          Buffers: shared hit=890
          ->  Sort (actual time=1.962..1.992 rows=827.00 loops=1)
                Sort Key: d.term
                Sort Method: quicksort  Memory: 47kB
                Buffers: shared hit=890
                ->  Result (actual time=0.060..1.859 rows=827.00 loops=1)
                      One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
                      Buffers: shared hit=890
                      ->  Nested Loop (actual time=0.058..1.796 rows=827.00 loops=1)
                            Buffers: shared hit=890
                            ->  CTE Scan on qt (actual time=0.000..0.001 rows=1.00 loops=1)
                                  Storage: Memory  Maximum Storage: 17kB
                            ->  Index Scan using bm25_df_pkey on bm25_df d (actual time=0.056..1.717 rows=827.00 loops=1)
                                  Index Cond: ((tenant_id = '<uuid>'::uuid) AND (term = ANY (qt.terms)) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                                  Buffers: shared hit=890
  CTE tf
    ->  Result (actual time=5.590..1663.770 rows=166792.00 loops=1)
          One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
```

### c_dual_or — exact query

```
Limit (actual time=119.761..119.764 rows=10.00 loops=1)
  ->  Sort (actual time=119.759..119.760 rows=10.00 loops=1)
        Sort Key: ((ts_rank(m.fts_english, '''pgbouncer'' | ''transact'' | ''mode'''::tsquery, 0) + ts_rank(m.fts_simple, '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery, 0))) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        ->  Result (actual time=0.350..119.271 rows=1681.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.319..117.601 rows=1681.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ((fts_english @@ '''pgbouncer'' | ''transact'' | ''mode'''::tsquery) OR (fts_simple @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery)))
                    Rows Removed by Filter: 63811
  Buffers: shared hit=622
Planning Time: 5.372 ms
Execution Time: 119.856 ms
```

### c_dual_or — long query

```
Limit (actual time=336.494..336.497 rows=10.00 loops=1)
  ->  Sort (actual time=336.493..336.494 rows=10.00 loops=1)
        Sort Key: ((ts_rank(m.fts_english, '''worker'' | ''kill'' | ''pdf'' | ''render'' | ''job'' | ''roll'' | ''deploy'' | ''invoic'' | ''end'' | ''half'' | ''render'' | ''stop'''::tsquery, 0) + ts_rank(m.fts_simple, '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0))) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        ->  Result (actual time=0.083..329.385 rows=59691.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.063..134.376 rows=59691.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ((fts_english @@ '''worker'' | ''kill'' | ''pdf'' | ''render'' | ''job'' | ''roll'' | ''deploy'' | ''invoic'' | ''end'' | ''half'' | ''render'' | ''stop'''::tsquery) OR (fts_simple @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery)))
                    Rows Removed by Filter: 5801
  Buffers: shared hit=622
Planning Time: 4.636 ms
Execution Time: 336.572 ms
```

### c_dual_or@no_rls — exact query

```
Limit (actual time=15.477..15.480 rows=10.00 loops=1)
  Buffers: shared hit=2354
  ->  Sort (actual time=15.477..15.479 rows=10.00 loops=1)
        Sort Key: ((ts_rank(m.fts_english, '''pgbouncer'' | ''transact'' | ''mode'''::tsquery, 0) + ts_rank(m.fts_simple, '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery, 0))) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=2354
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=7.842..15.265 rows=1681.00 loops=1)
              Recheck Cond: (((fts_english @@ '''pgbouncer'' | ''transact'' | ''mode'''::tsquery) OR (fts_simple @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery)) AND (tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
              Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])))
              Rows Removed by Filter: 142
              Heap Blocks: exact=1744
              Buffers: shared hit=2348
              ->  BitmapAnd (actual time=7.674..7.675 rows=0.00 loops=1)
                    ->  BitmapOr (actual time=0.898..0.899 rows=0.00 loops=1)
                          Buffers: shared hit=24
                          ->  Bitmap Index Scan on memories_t_<tenant>_fts_english_idx (actual time=0.599..0.600 rows=2792.00 loops=1)
                                Index Cond: (fts_english @@ '''pgbouncer'' | ''transact'' | ''mode'''::tsquery)
                                Buffers: shared hit=12
                          ->  Bitmap Index Scan on memories_t_<tenant>_fts_simple_idx (actual time=0.296..0.296 rows=2792.00 loops=1)
                                Index Cond: (fts_simple @@ '''pgbouncer'' | ''transaction'' | ''mode'''::tsquery)
                                Buffers: shared hit=12
                    ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=6.576..6.576 rows=65492.00 loops=1)
                          Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
  Buffers: shared hit=621
Planning Time: 2.040 ms
Execution Time: 15.562 ms
```

### c_dual_or@no_rls — long query

```
Limit (actual time=300.638..300.641 rows=10.00 loops=1)
  ->  Sort (actual time=300.637..300.638 rows=10.00 loops=1)
        Sort Key: ((ts_rank(m.fts_english, '''worker'' | ''kill'' | ''pdf'' | ''render'' | ''job'' | ''roll'' | ''deploy'' | ''invoic'' | ''end'' | ''half'' | ''render'' | ''stop'''::tsquery, 0) + ts_rank(m.fts_simple, '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery, 0))) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=0.032..294.531 rows=59691.00 loops=1)
              Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ((fts_english @@ '''worker'' | ''kill'' | ''pdf'' | ''render'' | ''job'' | ''roll'' | ''deploy'' | ''invoic'' | ''end'' | ''half'' | ''render'' | ''stop'''::tsquery) OR (fts_simple @@ '''the'' | ''worker'' | ''is'' | ''killing'' | ''pdf'' | ''render'' | ''jobs'' | ''during'' | ''a'' | ''rolling'' | ''deploy'' | ''and'' | ''invoices'' | ''end'' | ''up'' | ''half'' | ''rendered'' | ''how'' | ''do'' | ''we'' | ''stop'' | ''that'''::tsquery)))
              Rows Removed by Filter: 5801
  Buffers: shared hit=621
Planning Time: 1.952 ms
Execution Time: 300.659 ms
```

### e_trgm_text — exact query

```
Limit (actual time=1823.569..1823.572 rows=10.00 loops=1)
  ->  Sort (actual time=1823.568..1823.569 rows=10.00 loops=1)
        Sort Key: (word_similarity('PgBouncer transaction mode'::text, m.claim)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        ->  Result (actual time=1.354..1823.097 rows=688.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=1.323..1804.502 rows=688.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('PgBouncer transaction mode'::text <% claim))
                    Rows Removed by Filter: 65104
  Buffers: shared hit=621
Planning Time: 1.982 ms
Execution Time: 1823.619 ms
```

### e_trgm_text — long query

```
Limit (actual time=3083.192..3083.194 rows=0.00 loops=1)
  ->  Sort (actual time=3083.190..3083.191 rows=0.00 loops=1)
        Sort Key: (word_similarity('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text, m.claim)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        ->  Result (actual time=3083.074..3083.074 rows=0.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=3083.053..3083.053 rows=0.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text <% claim))
                    Rows Removed by Filter: 65792
  Buffers: shared hit=621
Planning Time: 3.735 ms
Execution Time: 3083.300 ms
```

### e_trgm_text@no_rls — exact query

```
Limit (actual time=88.717..88.721 rows=10.00 loops=1)
  Buffers: shared hit=1729
  ->  Sort (actual time=88.715..88.718 rows=10.00 loops=1)
        Sort Key: (word_similarity('PgBouncer transaction mode'::text, m.claim)) DESC, m.id
        Sort Method: top-N heapsort  Memory: 26kB
        Buffers: shared hit=1729
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=34.500..88.469 rows=688.00 loops=1)
              Recheck Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])) AND is_tip AND (status = 'promoted'::text))
              Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('PgBouncer transaction mode'::text <% claim))
              Rows Removed by Filter: 245
              Heap Blocks: exact=916
              Buffers: shared hit=1723
              ->  BitmapAnd (actual time=34.329..34.330 rows=0.00 loops=1)
                    Buffers: shared hit=807
                    ->  Bitmap Index Scan on memories_t_<tenant>_claim_idx (actual time=26.822..26.823 rows=1436.00 loops=1)
                          Index Cond: (claim %> 'PgBouncer transaction mode'::text)
                          Buffers: shared hit=267
                    ->  Bitmap Index Scan on memories_t_<tenant>_served_tip (actual time=7.315..7.315 rows=65792.00 loops=1)
                          Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
  Buffers: shared hit=622
Planning Time: 13.863 ms
Execution Time: 88.904 ms
```

### e_trgm_text@no_rls — long query

```
Limit (actual time=97.079..97.081 rows=0.00 loops=1)
  Buffers: shared hit=1175
  ->  Sort (actual time=97.078..97.079 rows=0.00 loops=1)
        Sort Key: (word_similarity('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text, m.claim)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=1175
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=97.048..97.049 rows=0.00 loops=1)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text <% claim) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Rows Removed by Filter: 225
              Heap Blocks: exact=225
              Buffers: shared hit=1169
              ->  Bitmap Index Scan on memories_t_<tenant>_claim_idx (actual time=85.517..85.518 rows=225.00 loops=1)
                    Index Cond: (claim %> 'The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text)
                    Buffers: shared hit=944
  Buffers: shared hit=622
Planning Time: 8.595 ms
Execution Time: 97.240 ms
```

### e_trgm_ident — exact query

```
Limit (actual time=389.208..389.209 rows=0.00 loops=1)
  ->  Sort (actual time=389.206..389.207 rows=0.00 loops=1)
        Sort Key: (word_similarity('PgBouncer transaction mode'::text, m.idents)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        ->  Result (actual time=389.178..389.179 rows=0.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=389.175..389.175 rows=0.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('PgBouncer transaction mode'::text <% idents))
                    Rows Removed by Filter: 65792
  Buffers: shared hit=618
Planning Time: 1.944 ms
Execution Time: 389.264 ms
```

### e_trgm_ident — long query

```
Limit (actual time=1075.735..1075.737 rows=0.00 loops=1)
  ->  Sort (actual time=1075.734..1075.735 rows=0.00 loops=1)
        Sort Key: (word_similarity('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text, m.idents)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        ->  Result (actual time=1075.693..1075.693 rows=0.00 loops=1)
              One-Time Filter: ((NULLIF(current_setting('oml.tenant_id'::text, true), ''::text))::uuid = '<uuid>'::uuid)
              ->  Index Scan using memories_t_<tenant>_served_tip on memories_t_<tenant> m (actual time=1075.687..1075.688 rows=0.00 loops=1)
                    Index Cond: ((tenant_id = '<uuid>'::uuid) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
                    Filter: ((NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text <% idents))
                    Rows Removed by Filter: 65792
  Buffers: shared hit=618
Planning Time: 3.091 ms
Execution Time: 1075.813 ms
```

### e_trgm_ident@no_rls — exact query

```
Limit (actual time=5.286..5.288 rows=0.00 loops=1)
  Buffers: shared hit=147
  ->  Sort (actual time=5.284..5.286 rows=0.00 loops=1)
        Sort Key: (word_similarity('PgBouncer transaction mode'::text, m.idents)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        Buffers: shared hit=147
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=5.243..5.243 rows=0.00 loops=1)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('PgBouncer transaction mode'::text <% idents) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Buffers: shared hit=141
              ->  Bitmap Index Scan on memories_t_<tenant>_idents_idx (actual time=5.234..5.234 rows=0.00 loops=1)
                    Index Cond: (idents %> 'PgBouncer transaction mode'::text)
                    Buffers: shared hit=141
  Buffers: shared hit=619
Planning Time: 5.724 ms
Execution Time: 5.514 ms
```

### e_trgm_ident@no_rls — long query

```
Limit (actual time=19.606..19.608 rows=0.00 loops=1)
  ->  Sort (actual time=19.604..19.605 rows=0.00 loops=1)
        Sort Key: (word_similarity('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text, m.idents)) DESC, m.id
        Sort Method: quicksort  Memory: 25kB
        ->  Bitmap Heap Scan on memories_t_<tenant> m (actual time=19.580..19.581 rows=0.00 loops=1)
              Filter: (is_tip AND (NOT retirement) AND (kind = ANY ('{memory,derived_fact}'::text[])) AND ('The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text <% idents) AND (tenant_id = '<uuid>'::uuid) AND (status = 'promoted'::text) AND (workarea_id = ANY ('{…readable workareas…}'::uuid[])))
              Buffers: shared hit=494
              ->  Bitmap Index Scan on memories_t_<tenant>_idents_idx (actual time=19.565..19.566 rows=0.00 loops=1)
                    Index Cond: (idents %> 'The worker is killing PDF render jobs during a rolling deploy and invoices end up half rendered, how do we stop that?'::text)
                    Buffers: shared hit=494
  Buffers: shared hit=619
Planning Time: 5.330 ms
Execution Time: 19.846 ms
```
