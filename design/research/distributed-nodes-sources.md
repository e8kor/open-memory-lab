<!--
id: engineering/research/distributed-nodes-sources
tier: engineering
status: sources for a proposed record
answers: the primary-source passages ADR-0073 rests on, quoted, with the date each was fetched
-->

# Distributed nodes: sources

The passages [ADR-0073](../../docs/adr/ADR-0073-a-leader-signs-every-write-and-nodes-serve-the-workareas-placed-on-them-each-read-routed-to-the-closest-less-busy-replica-fresh-enough-for-it.md)
rests on, quoted from the primary source. The record keeps one line per source and links here.
Fetched 2026-11-08 unless marked 2026-11-10. A source marked "not fetched" is cited from memory and
carries no quotation.

## PostgreSQL

- **Synchronous commit** ([log-shipping standby servers](https://www.postgresql.org/docs/17/warm-standby.html),
  §26.2.8, fetched 2026-11-10): "each commit of a write transaction will wait until confirmation is
  received that the commit has been written to the write-ahead log on disk of both the primary and
  standby server". §26.2.8.4: "Such transaction commits may never be completed if any one of the
  synchronous standbys should crash." "The best solution for high availability is to ensure you
  keep as many synchronous standbys as requested." "In a quorum-based synchronous replication, all
  the standbys appearing in the list will be used as candidates for synchronous standbys. Even if
  one of them should fail, the other standbys will keep performing the role of candidates of
  synchronous standby." "If you really cannot keep as many synchronous standbys as requested then
  you should decrease the number of synchronous standbys that transaction commits must wait for
  responses from in `synchronous_standby_names` (or disable it)". Fetched 2026-11-08: "Some
  transactions may not show as committed on the standby, even though they show as committed on the
  primary". ADR-0073 d5.
- **`synchronous_standby_names`** ([replication settings](https://www.postgresql.org/docs/17/runtime-config-replication.html),
  fetched 2026-11-10): "The keyword `ANY`, coupled with `num_sync`, specifies a quorum-based
  synchronous replication and makes transaction commits wait until their WAL records are replicated
  to at least `num_sync` listed standbys." "If no synchronous standby names are specified here, then
  synchronous replication is not enabled and transaction commits will not wait for replication."
  "Even when synchronous replication is enabled, individual transactions can be configured not to
  wait for replication by setting the `synchronous_commit` parameter to `local` or `off`." ADR-0073
  d5.
- **Logical replication** ([restrictions](https://www.postgresql.org/docs/17/logical-replication-restrictions.html),
  [row filters](https://www.postgresql.org/docs/17/logical-replication-row-filter.html),
  [security](https://www.postgresql.org/docs/17/logical-replication-security.html)): "a row is
  replicated only if its data satisfies the row filter expression"; "The `WHERE` clause allows only
  simple expressions"; "The database schema and DDL commands are not replicated"; apply "will switch
  roles to the table owner"; "Sequence data is not replicated". ADR-0073 d5's comparison.
- **Hot standby** ([hot standby](https://www.postgresql.org/docs/17/hot-standby.html)): "Conflicting
  queries will be canceled once it has taken longer than the relevant delay setting". A standby is
  restored from "a base backup of the primary". ADR-0073 d5's comparison.
- **Failover** ([failover](https://www.postgresql.org/docs/17/warm-standby-failover.html)):
  "PostgreSQL does not provide the system software required to identify a failure on the primary and
  notify the standby database server"; fencing the old primary is "necessary to avoid situations
  where both systems think they are the primary". ADR-0073 d10.

## Clocks and leases

- **clock_gettime(2)** ([man7](https://man7.org/linux/man-pages/man2/clock_gettime.2.html)):
  CLOCK_BOOTTIME "is identical to CLOCK_MONOTONIC, except that it also includes any time that the
  system is suspended"; CLOCK_MONOTONIC "does not count time that the system is suspended" and "is
  affected by frequency adjustments". ADR-0073 d8 C1.
- **Raft** (Ongaro and Ousterhout, *In Search of an Understandable Consensus Algorithm*, extended
  version, §8): lease-based reads "rely on timing for safety (it assumes bounded clock skew)".
  ADR-0073 d8.
- **Fencing tokens** (Kleppmann, *How to do distributed locking*, 2016, not fetched). ADR-0073 d8.
- **Leases** (Gray and Cheriton, 1989, not fetched). ADR-0073 d8.

## Placement and routing

- **Rendezvous hashing** ([article](https://en.wikipedia.org/wiki/Rendezvous_hashing), after Thaler
  and Ravishankar, IEEE/ACM ToN 1998): clients "achieve distributed agreement" on k of n options,
  each picking "the k sites that yield the k largest hash values"; "When a site fails, only the
  objects mapped to that site need to be remapped". The weighting `w / −ln(h)` is the article's,
  after Schindelhauer and Schomaker, SPAA 2005 (not fetched). Bounded loads: Mirrokni, Thorup and
  Zadimoghaddam, SODA 2018 (not fetched). ADR-0073 d3.
- **Envoy** ([outlier detection](https://www.envoyproxy.io/docs/envoy/latest/intro/arch_overview/upstream/outlier)):
  ejection for "the outlier_detection.base_ejection_time value multiplied by the number of times the
  host has been ejected in a row".
  [Load balancers](https://www.envoyproxy.io/docs/envoy/latest/intro/arch_overview/upstream/load_balancing/load_balancers):
  least request samples "N hosts", default 2, and "picks the host which has the fewest active
  requests". [Zone aware routing](https://www.envoyproxy.io/docs/envoy/latest/intro/arch_overview/upstream/load_balancing/zone_aware):
  "tries to push as much traffic as possible to the local upstream zone", sending the rest by "the
  percentage of upstream hosts". ADR-0073 d6.
- **Finagle** ([clients](https://twitter.github.io/finagle/guide/Clients.html)): "Peak EWMA uses a
  moving average over an endpoint's round-trip time (RTT) that is highly sensitive to peaks",
  "weighted by the number of outstanding requests". ADR-0073 d6.
- **Two choices** (Mitzenmacher, *The Power of Two Choices in Randomized Load Balancing*, IEEE TPDS
  12(10), 2001, abstract): "Having d ≥ 2 choices leads to exponential improvements in the expected
  time a customer spends in the system over d = 1, whereas having d = 3 choices is only a constant
  factor better than d = 2". ADR-0073 d6.
- **Hedged requests** (Dean and Barroso, *The Tail at Scale*, CACM 56(2), 2013, not fetched).
  ADR-0073 d9.

## Bounded staleness

- **CockroachDB** ([follower reads](https://docs.cockroachlabs.com/docs/stable/follower-reads)):
  `with_min_timestamp` "Defines a minimum timestamp at which to perform the bounded staleness read";
  `with_max_staleness(INTERVAL)` "is equivalent to `with_min_timestamp(now() - INTERVAL)`"; a closed
  timestamp is one below which "no new writes can ever be introduced". ADR-0073 d7.
- **Spanner** ([reads](https://docs.cloud.google.com/spanner/docs/reads)): strong reads "observe the
  effects of all transactions that committed before the start of the operation, independent of
  which replica receives the read"; stale reads are "read at a timestamp in the past". ADR-0073 d7.
