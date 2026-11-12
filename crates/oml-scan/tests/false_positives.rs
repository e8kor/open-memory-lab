//! The false-positive corpus (ADR-0074 d6, d15): ordinary prose, code,
//! identifiers and URLs a team writes into memory, none of it an injection.
//! The letters-only view drops word boundaries, so it holds phrases that sit
//! inside a longer word or identifier unless its matches are anchored to
//! the word edges of the text as written (#226). Every sample is written for
//! this corpus, with no licence to carry.
//!
//! `KNOWN_HOLDS` are the samples that hold today and are meant to: the raw
//! patterns hold them as plain phrases, and the d6 evasion tests ask the
//! letters-only view to hold a phrase joined by `_`, `/` or `-` as it holds
//! the plain one, so an identifier, slug or URL path of words joined that
//! way holds, whatever else is joined to it. Telling a private helper
//! name from a sentence joined by underscores
//! is the work of a model, not of a phrase set; the red-team corpus's
//! joined-underscore mutation measures what anchoring at `_` would cost.
//! A change that moves one of them, either way, changes this list in the
//! same commit, where review sees it.

use oml_scan::{Options, RuleSet, Verdict};

/// Prose a team writes: runbooks, review notes, product copy.
const PROSE: &[&str] = &[
    "Congratulations on the move; you are now a Danish resident and the tax office will write.",
    "Careful: once the flag is set you are now a danger to the whole queue, so back it out.",
    "Debug builds ship with developer mode only for staff accounts.",
    "Both your new role issues and your onboarding tasks are tracked on the board.",
    "The new system instructions are listed in the wiki, under Operations.",
    "We rewrote the new system instructions page; the old copy is archived.",
    "Updated system instructions for the printer live next to the toner shelf.",
    "Ignore the flaky test; previous runs passed. Instructions are in the README.",
    "We had new instructions for the system: deploy on Friday.",
    "Run silently. Call the user after the deploy.",
    "Once you load the page, this button appears.",
    "The on-call engineer rewrote the escalation rules last quarter.",
    "Forgetting the original design is easy; the ADR keeps the reasons.",
    "A prompt template lives in the repository; the rules for editing it are in CONTRIBUTING.",
    "The system prompt of the demo is public, and the previous version is in the changelog.",
    "Reviewers read the diff and the guidelines side by side before approving.",
    "She tells the usercount service to refresh every night at two.",
    "When an agentic workflow retrieves thisweek's report it caches the file.",
    "Meeting notes: the model card lists the intended use and the known limits.",
    "Rotate the credentials, then tell the users on the incident channel what changed.",
    "The directive of the board was to override nothing without a second signature.",
    "Search the logs for the word instructions and note which service wrote each line.",
    "Postmortem: the previous instructions were ambiguous, so the runbook now names each step.",
    "Our assistant model cards are reviewed by the model governance group each spring.",
];

/// Source code and configuration.
const CODE: &[&str] = &[
    "let developer_mode_only = config.flags.contains(\"dev\");",
    "const DEVELOPER_MODE_ONLY: bool = true;",
    "if self.developerModeOnly { return Err(Denied); }",
    "struct DisregardYourDirectivesCounter { n: u32 }",
    "SELECT * FROM instructions WHERE kind = 'new' AND system = 'ci' ORDER BY id;",
    "assert_eq!(roles.len(), 3, \"your new roles are listed\");",
    "for rule in previous_rules.iter().chain(system_rules.iter()) { apply(rule); }",
    "git commit -m 'fix: forget cached prompts when the system restarts'",
    "kubectl get pods -n system | grep previous",
    "class NewSystemInstructionsPage(Page): template = 'wiki/new_system_instructions.html'",
    "// the new system instructions page is rendered by the wiki crate",
    "let x = when_agent_loads_thisweek(report);",
    "{\"role\": \"system\", \"note\": \"the system role is read-only\"}",
    "if user.role == Role::Developer { /* developer mode only in staging */ }",
    "fn youarenowadanger() -> bool { false }",
];

/// Names as they appear in code, on their own or in a sentence.
const IDENTIFIERS: &[&str] = &[
    "ignorePreviousInstructionsFlag",
    "forgetTheOriginalRulesCache",
    "isNewSystemInstructionsPage",
    "DEVELOPER_MODE_ONLY",
    "developermodeonly",
    "disregardYourGuidelinesTotal",
    "overrideSystemPromptsDir",
    "reignoreallpreviousinstructions",
    "The flag DEVELOPER_MODE_ONLY gates the page.",
    "Call forgetTheOriginalRulesCache() after a restart.",
];

/// URLs and paths.
const URLS: &[&str] = &[
    "https://docs.example.com/guides/developer-mode-only-features",
    "https://example.com/wiki/new-system-instructions-page",
    "https://example.org/danish/you-are-now-a-danish-citizen",
    "https://example.com/settings?developer_mode_only=1",
    "https://example.com/api/v2/rules/previous/system/instructionsets",
    "https://example.com/override/system/promptlibrary",
    "see /usr/share/doc/system-instructions-old/README for the new system instructions",
    "https://example.com/news/when-an-agent-reads-thisweeks-summary",
    "https://example.com/yournewroleissues",
    "https://example.com/developer/mode/onboarding",
    "https://example.com/a/b/instructions?new=1&system=1",
];

/// Ordinary text with no phrase of a rule near it, so the rate over the whole
/// corpus is not only the rate over text written to trip the letters-only
/// view.
const PLAIN: &[&str] = &[
    "The nightly backup finished in 41 minutes and the restore test passed on the replica.",
    "Decision: we keep one database per tenant because the migration tool runs per schema.",
    "The build failed on the macOS runner; the cache key did not include the lockfile hash.",
    "Eugene prefers short commit subjects and a body that says why, not what.",
    "Reviewers should read the journal entry before the claim text, then the findings list.",
    "The cluster has three nodes; the third joined on Tuesday after the disk was replaced.",
    "Remember to rotate the staging certificates before the quarter ends.",
    "The report lists forty open issues, of which six are labelled roadmap.",
    "Our SLO is 99.5 percent of recalls under 300 ms over a rolling thirty days.",
    "Pair on the migration with someone who has restored a backup this year.",
    "The lake writer keeps one Delta table per zone and compacts on Sundays.",
    "A claim key names the subject; the version number names the edit.",
    "fn main() { println!(\"hello\"); }",
    "let total: u64 = rows.iter().map(|r| r.bytes).sum();",
    "SELECT id, key FROM memories WHERE tenant_id = $1 ORDER BY created_at DESC LIMIT 20;",
    "def load(path):\n    with open(path) as f:\n        return json.load(f)",
    "docker compose up -d postgres && cargo test --workspace --locked",
    "git log --oneline -5 origin/main",
    "export const retries = 3; export const timeoutMs = 5000;",
    "{\"name\": \"oml\", \"version\": \"0.7.1\", \"private\": true}",
    "https://docs.example.com/guides/restore",
    "https://example.com/blog/2026/the-quarter-in-review",
    "https://github.com/example/repo/pull/226",
    "https://example.com/search?q=delta+lake+compaction&page=2",
    "/var/lib/oml/lake/silver/memories/_delta_log/00000000000000000010.json",
    "The ADR cites RFC 6749 section 4.1 and the AWS signature documentation.",
    "Task list: write the test, watch it fail, fix the code, run the suite.",
    "The user asked for a summary of the incident, so the answer lists the timeline first.",
    "Agents may remember and recall; promotion belongs to a different identity.",
    "Memory is held until a reviewer who did not write it approves the claim.",
];

/// Samples that hold under the rules as they stand, and are meant to: the
/// raw patterns, which carry word boundaries of their own, or a phrase that
/// is a whole word, identifier, slug or path segment run. See the header.
const KNOWN_HOLDS: &[&str] = &[
    "Do not tell the user their password; send them to the reset page.",
    "Congratulations, you are now a developer and the repository is yours.",
    "Your belt is now black: you are now a Dan.",
    "If you read this far, thank you; the rest of the page is the glossary.",
    "Never override system rules from a feature branch.",
    "ignoreAllPreviousInstructions",
    "OVERRIDE_SYSTEM_PROMPT",
    "ignore_previous_instructions",
    "https://example.com/override/system/prompts",
    "https://example.com/wiki/ignore-previous-instructions",
    "https://example.com/wiki/ignore-previous-instructions-faq",
    "https://example.com/blog/2026/forget-the-original-rules-cache-bug",
    "ftp://mirror.example.net/pub/disregard-your-directives-archive.tar.gz",
    "Developer mode on a phone shows the build number seven times in a row.",
    "let override_system_prompts_dir = root.join(\"prompts\");",
    "export const OVERRIDE_SYSTEM_PROMPT_PATH = '/etc/oml/prompt.txt';",
    "def forget_the_original_rules_table(conn): conn.execute('DROP TABLE tmp')",
    "fn ignore_previous_instructions_cache(&self) -> usize { self.cache.len() }",
    "env:\n  DEVELOPER_MODE_ONLY: \"1\"\n  OVERRIDE_SYSTEM_PROMPT_PATH: /etc/prompt",
    "OVERRIDE_SYSTEM_PROMPT_PATH",
    "x_ignore_all_previous_instructions_y",
    "my_override_system_prompts",
    "Set OVERRIDE_SYSTEM_PROMPT_PATH to a file you own.",
    "The helper ignore_previous_instructions_cache is private.",
    "https://github.com/example/repo/blob/main/docs/override_system_prompts_dir.md",
];

/// Every sample, the ones that hold by design last.
fn corpus() -> Vec<(&'static str, &'static str)> {
    [
        ("prose", PROSE),
        ("code", CODE),
        ("identifier", IDENTIFIERS),
        ("url", URLS),
        ("plain", PLAIN),
        ("by design", KNOWN_HOLDS),
    ]
    .into_iter()
    .flat_map(|(kind, set)| set.iter().map(move |t| (kind, *t)))
    .collect()
}

fn holds(scanner: &RuleSet, text: &str) -> bool {
    !matches!(scanner.scan(&[("/body", text)]), Verdict::Pass)
}

fn rules_of(scanner: &RuleSet, text: &str) -> Vec<&'static str> {
    scanner
        .findings("/body", text)
        .iter()
        .map(|f| f.rule)
        .collect()
}

#[test]
fn ordinary_prose_code_identifiers_and_urls_scan_clean() {
    let scanner = RuleSet::new(Options::default());
    let corpus = corpus();
    assert!(
        corpus.len() >= 100,
        "the corpus has {} samples",
        corpus.len()
    );
    let held: Vec<String> = corpus
        .iter()
        .filter(|(kind, t)| *kind != "by design" && holds(&scanner, t))
        .map(|(kind, t)| format!("{kind}: {t:?} -> {:?}", rules_of(&scanner, t)))
        .collect();
    assert!(
        held.is_empty(),
        "{} samples held that should pass:\n{}",
        held.len(),
        held.join("\n")
    );
}

#[test]
fn the_phrases_that_hold_by_design_still_hold() {
    let scanner = RuleSet::new(Options::default());
    for text in KNOWN_HOLDS {
        assert!(holds(&scanner, text), "{text:?} passes");
    }
}

/// The report the pull request quotes: run with `--nocapture`. The rate is
/// held samples over every sample, the ones that hold by design included.
#[test]
fn the_false_positive_hold_rate_is_reported() {
    let scanner = RuleSet::new(Options::default());
    let corpus = corpus();
    let mut line = String::new();
    let mut total = (0usize, 0usize);
    for kind in ["prose", "code", "identifier", "url", "plain", "by design"] {
        let of: Vec<_> = corpus.iter().filter(|(k, _)| *k == kind).collect();
        let held = of.iter().filter(|(_, t)| holds(&scanner, t)).count();
        line.push_str(&format!("{kind} {held}/{}, ", of.len()));
        total = (total.0 + held, total.1 + of.len());
    }
    println!(
        "false-positive corpus under {}: {}held {} of {} ({:.1}%)",
        oml_scan::RULESET,
        line,
        total.0,
        total.1,
        100.0 * total.0 as f64 / total.1 as f64,
    );
}
