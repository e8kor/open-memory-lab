//! Guardrail rules: severity, the `oml-gr/1` rule grammar, the tool classes
//! and their argument normalisation, the linear-time glob, the check before
//! a tool call and the rule cache's codec (ADR-0037 d2, d7, d8,
//! d9; the leaf crate of d18 Q10).

use oml_guardrail::{
    CACHE_MAX_AGE_DAYS, CacheRefusal, CacheState, CachedRule, Cause, Effect, GRAMMAR, Glob,
    NO_CACHE_WARNING, OWN_HELD_MAX_LINES, PROVENANCE_MAX_CHARS, Rule, SET_CAP, Severity,
    ShellRefusal, ToolCall, ToolClass, Uncheckable, Verdict, cache_file_name, decide, decode_cache,
    encode_cache, normalise_path, shell_segments, url_host,
};
use proptest::prelude::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const CACHE_DIR: &str = "/home/u/.local/state/oml/guardrails";
const CWD: &str = "/work/repo";

fn rule(value: Value) -> Rule {
    match Rule::parse(&value) {
        Ok(rule) => rule,
        Err(defect) => panic!("{value} was refused: {defect}"),
    }
}

fn cached(id: u128, body: &str, value: Value) -> CachedRule {
    CachedRule {
        artifact_id: Uuid::from_u128(id),
        body: body.to_owned(),
        rule: rule(value),
    }
}

fn verdict(rules: &[CachedRule], tool_name: &str, tool_input: Value) -> Verdict {
    decide(
        &CacheState::Loaded(rules.to_vec()),
        &ToolCall {
            tool_name,
            tool_input: &tool_input,
            cwd: Some(CWD),
        },
        CACHE_DIR,
    )
}

fn effect(v: &Verdict) -> Option<Effect> {
    match v {
        Verdict::Decide(decision) => Some(decision.effect),
        Verdict::Nothing | Verdict::Warn(_) => None,
    }
}

fn shell(rules: &[CachedRule], command: &str) -> Option<Effect> {
    effect(&verdict(rules, "Bash", json!({ "command": command })))
}

fn words(segments: &[&[&str]]) -> Vec<Vec<String>> {
    segments
        .iter()
        .map(|s| s.iter().map(|w| (*w).to_owned()).collect())
        .collect()
}

fn deny_push() -> CachedRule {
    cached(
        1,
        "Never push from an agent session.",
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["git", "push", "*"], "effect": "deny"}),
    )
}

// --- d2, d10: the kind's fields, its severity and its caps ---------------

#[test]
fn a_severity_is_never_must_or_should_and_nothing_else() {
    for (word, severity) in [
        ("never", Severity::Never),
        ("must", Severity::Must),
        ("should", Severity::Should),
    ] {
        assert_eq!(Severity::parse(word), Some(severity));
        assert_eq!(severity.as_str(), word);
    }
    for word in ["", "Never", "MUST", "may", "priority", "high"] {
        assert_eq!(
            Severity::parse(word),
            None,
            "{word:?} is refused, never coerced"
        );
    }
}

#[test]
fn the_caps_are_the_design_constants_derived_from_one_sourced_cap() {
    assert_eq!(PROVENANCE_MAX_CHARS, 200);
    assert_eq!(OWN_HELD_MAX_LINES, 5);
    assert_eq!(SET_CAP, 12);
    assert_eq!(CACHE_MAX_AGE_DAYS, 30);
    assert_eq!(GRAMMAR, "oml-gr/1");
}

// --- d4: the order and conflicts ----------------------------------------

#[test]
fn each_mapped_tool_is_matched_on_its_argument_and_no_other_tool_by_any_rule() {
    for (name, class) in [
        ("Bash", ToolClass::Shell),
        ("PowerShell", ToolClass::Shell),
        ("Write", ToolClass::Edit),
        ("Edit", ToolClass::Edit),
        ("MultiEdit", ToolClass::Edit),
        ("NotebookEdit", ToolClass::Edit),
        ("Read", ToolClass::Read),
        ("Grep", ToolClass::Read),
        ("Glob", ToolClass::Read),
        ("WebFetch", ToolClass::Fetch),
        ("mcp__github__create_issue", ToolClass::Mcp),
    ] {
        assert_eq!(ToolClass::of(name), Some(class), "{name}");
    }
    for name in [
        "WebSearch",
        "Agent",
        "Task",
        "TodoWrite",
        "bash",
        "read",
        "Fetch",
    ] {
        assert_eq!(ToolClass::of(name), None, "{name} is matched by no rule");
    }

    let every_class = [
        cached(
            1,
            "no shell",
            json!({"rule": "oml-gr/1", "tool": "shell", "command": ["*", "*"], "effect": "deny"}),
        ),
        cached(
            2,
            "no edit",
            json!({"rule": "oml-gr/1", "tool": "edit", "path": "/**", "effect": "deny"}),
        ),
        cached(
            3,
            "no read",
            json!({"rule": "oml-gr/1", "tool": "read", "path": "/**", "effect": "deny"}),
        ),
        cached(
            4,
            "no fetch",
            json!({"rule": "oml-gr/1", "tool": "fetch", "host": "*.com", "effect": "deny"}),
        ),
        cached(
            5,
            "no mcp",
            json!({"rule": "oml-gr/1", "tool": "mcp", "name": "*", "effect": "deny"}),
        ),
    ];
    let denied_by = |tool: &str, input: Value| match verdict(&every_class, tool, input) {
        Verdict::Decide(d) if d.effect == Effect::Deny => d
            .causes
            .iter()
            .map(|c| match c {
                Cause::Rule { artifact_id, .. } => artifact_id.as_u128(),
                other => panic!("{other:?}"),
            })
            .collect::<Vec<_>>(),
        other => panic!("{tool}: {other:?}"),
    };
    assert_eq!(denied_by("Bash", json!({"command": "ls -l"})), vec![1]);
    assert_eq!(denied_by("Write", json!({"file_path": "/a/b"})), vec![2]);
    assert_eq!(denied_by("Edit", json!({"file_path": "/a/b"})), vec![2]);
    assert_eq!(denied_by("Read", json!({"file_path": "/a/b"})), vec![3]);
    assert_eq!(
        denied_by("Grep", json!({"path": "/a", "pattern": "x"})),
        vec![3]
    );
    assert_eq!(
        denied_by("Glob", json!({"pattern": "*.rs"})),
        vec![3],
        "Glob falls back to cwd"
    );
    assert_eq!(
        denied_by("WebFetch", json!({"url": "https://a.com/x"})),
        vec![4]
    );
    assert_eq!(denied_by("mcp__x__y", json!({})), vec![5]);
    for tool in ["WebSearch", "Agent", "Task"] {
        assert_eq!(
            verdict(
                &every_class,
                tool,
                json!({"command": "ls", "file_path": "/a", "url": "https://a.com"})
            ),
            Verdict::Nothing,
            "{tool}"
        );
    }
    // The argument is the one the table names, never another field.
    let edit_only = [cached(
        2,
        "no edit",
        json!({"rule": "oml-gr/1", "tool": "edit", "path": "/secret/*", "effect": "deny"}),
    )];
    assert_eq!(
        verdict(
            &edit_only,
            "Write",
            json!({"file_path": "/open/a", "content": "/secret/x"})
        ),
        Verdict::Nothing
    );
}

#[test]
fn a_powershell_call_asks_whenever_a_shell_rule_is_cached() {
    let rules = [deny_push()];
    assert_eq!(
        verdict(&rules, "PowerShell", json!({"command": "Get-ChildItem"})),
        Verdict::Decide(oml_guardrail::Decision {
            effect: Effect::Ask,
            causes: vec![Cause::Uncheckable(Uncheckable::PowerShell)],
        })
    );
    assert_eq!(
        verdict(&[], "PowerShell", json!({"command": "Get-ChildItem"})),
        Verdict::Nothing,
        "with no shell rule cached, nothing is asked"
    );
}

#[test]
fn a_mapped_call_lacking_its_argument_asks_whenever_a_rule_of_its_class_is_cached() {
    let rules = [
        deny_push(),
        cached(
            2,
            "e",
            json!({"rule": "oml-gr/1", "tool": "edit", "path": "/x", "effect": "deny"}),
        ),
        cached(
            3,
            "r",
            json!({"rule": "oml-gr/1", "tool": "read", "path": "/x", "effect": "deny"}),
        ),
        cached(
            4,
            "f",
            json!({"rule": "oml-gr/1", "tool": "fetch", "host": "x.example", "effect": "deny"}),
        ),
    ];
    for (tool, input) in [
        ("Bash", json!({})),
        ("Bash", json!({"command": 3})),
        ("Write", json!({"content": "x"})),
        ("NotebookEdit", json!({"notebook_path": "/x"})),
        ("Read", json!({})),
        ("WebFetch", json!({})),
        ("WebFetch", json!({"url": "not a url"})),
    ] {
        match verdict(&rules, tool, input.clone()) {
            Verdict::Decide(d) => {
                assert_eq!(d.effect, Effect::Ask, "{tool} {input}");
                assert!(
                    matches!(d.causes[..], [Cause::Uncheckable(_)]),
                    "{tool}: {d:?}"
                );
            }
            other => panic!("{tool} {input}: {other:?}"),
        }
    }
    // A relative path with no cwd cannot be made absolute: it lacks its argument.
    let no_cwd = decide(
        &CacheState::Loaded(rules.to_vec()),
        &ToolCall {
            tool_name: "Read",
            tool_input: &json!({"file_path": "a"}),
            cwd: None,
        },
        CACHE_DIR,
    );
    assert_eq!(effect(&no_cwd), Some(Effect::Ask));
    // No rule of the class: nothing is asked.
    assert_eq!(verdict(&[deny_push()], "Read", json!({})), Verdict::Nothing);
}

#[test]
fn a_shell_segment_after_a_control_operator_is_matched() {
    let rules = [deny_push()];
    for command in [
        "git push",
        "git push origin main",
        "make test && git push",
        "make test || git push",
        "make test; git push",
        "make test & git push",
        "echo x | git push",
        "make test\ngit push",
        "make test;git push --force",
        "cd x && make 2>&1 && git push",
    ] {
        assert_eq!(shell(&rules, command), Some(Effect::Deny), "{command:?}");
    }
    for command in ["git status", "echo git push", "git pull && make", "gitpush"] {
        assert_eq!(shell(&rules, command), None, "{command:?}");
    }
    assert_eq!(
        shell_segments("a 1 && b; c | d & e || f\ng").expect("tokenises"),
        words(&[&["a", "1"], &["b"], &["c"], &["d"], &["e"], &["f"], &["g"]])
    );
    // Redirections are not words: their operator and target are dropped.
    assert_eq!(
        shell_segments("git push >log 2>&1 <in").expect("tokenises"),
        words(&[&["git", "push"]])
    );
}

#[test]
fn a_quoted_operator_splits_no_segment() {
    assert_eq!(
        shell_segments(r#"echo 'a; git push' "b && git push" c\;git push"#).expect("tokenises"),
        words(&[&["echo", "a; git push", "b && git push", "c;git", "push"]])
    );
    let rules = [deny_push()];
    assert_eq!(shell(&rules, "echo 'x; git push'"), None);
    assert_eq!(shell(&rules, r#"echo "x && git push""#), None);
    // Quoting removes the quotes and expands nothing.
    assert_eq!(
        shell_segments(r#"g'i't "pu"sh $HOME '$x' "\$y \"z\"""#).expect("tokenises"),
        words(&[&["git", "push", "$HOME", "$x", "$y \"z\""]])
    );
    assert_eq!(shell(&rules, "'git' \"push\""), Some(Effect::Deny));
    // A comment is not a segment.
    assert_eq!(
        shell_segments("make # && git push").expect("tokenises"),
        words(&[&["make"]])
    );
}

#[test]
fn a_command_that_does_not_tokenise_or_holds_a_substitution_asks() {
    for (command, refusal) in [
        ("echo 'unterminated", ShellRefusal::UnterminatedQuote),
        ("echo \"unterminated", ShellRefusal::UnterminatedQuote),
        ("echo trailing\\", ShellRefusal::TrailingEscape),
        ("echo $(git push)", ShellRefusal::CommandSubstitution),
        ("echo \"$(git push)\"", ShellRefusal::CommandSubstitution),
        ("echo `git push`", ShellRefusal::CommandSubstitution),
        ("echo \"`git push`\"", ShellRefusal::CommandSubstitution),
        ("echo $((1+2))", ShellRefusal::CommandSubstitution),
        ("diff <(ls) >(cat)", ShellRefusal::ProcessSubstitution),
        ("(git push)", ShellRefusal::Subshell),
        ("{ git push; }", ShellRefusal::BraceGroup),
        ("cat <<EOF\ngit push\nEOF", ShellRefusal::HereDocument),
        ("cat <<-EOF", ShellRefusal::HereDocument),
        ("echo >", ShellRefusal::MissingRedirectTarget),
    ] {
        assert_eq!(shell_segments(command), Err(refusal), "{command:?}");
        assert_eq!(
            verdict(&[deny_push()], "Bash", json!({ "command": command })),
            Verdict::Decide(oml_guardrail::Decision {
                effect: Effect::Ask,
                causes: vec![Cause::Uncheckable(Uncheckable::Shell(refusal))],
            }),
            "{command:?}"
        );
        assert_eq!(
            verdict(&[], "Bash", json!({ "command": command })),
            Verdict::Nothing,
            "with no shell rule cached, {command:?} asks nothing"
        );
    }
    // Quoted, the same characters are text.
    assert!(shell_segments("echo '$(x)' '`y`' '(z)' '{' '<<EOF'").is_ok());
    assert!(
        shell_segments("echo {a,b} x{").is_ok(),
        "a brace in a word is no group"
    );
}

#[test]
fn a_compound_command_or_a_dollar_quote_asks() {
    for (command, refusal) in [
        ("if git push; then :; fi", ShellRefusal::CompoundCommand),
        (
            "while true; do git push; done",
            ShellRefusal::CompoundCommand,
        ),
        (
            "until false; do git push; done",
            ShellRefusal::CompoundCommand,
        ),
        (
            "for x in 1; do git push; done",
            ShellRefusal::CompoundCommand,
        ),
        (
            "select x in a; do git push; done",
            ShellRefusal::CompoundCommand,
        ),
        (
            "case x in x) git push;; esac",
            ShellRefusal::CompoundCommand,
        ),
        ("! git push", ShellRefusal::CompoundCommand),
        ("time git push", ShellRefusal::CompoundCommand),
        ("coproc git push", ShellRefusal::CompoundCommand),
        ("make && then git push", ShellRefusal::CompoundCommand),
        ("$'git' push", ShellRefusal::DollarQuote),
        ("$\"git\" push", ShellRefusal::DollarQuote),
        ("git $'push'", ShellRefusal::DollarQuote),
    ] {
        assert_eq!(shell_segments(command), Err(refusal), "{command:?}");
        assert_eq!(
            verdict(&[deny_push()], "Bash", json!({ "command": command })),
            Verdict::Decide(oml_guardrail::Decision {
                effect: Effect::Ask,
                causes: vec![Cause::Uncheckable(Uncheckable::Shell(refusal))],
            }),
            "{command:?}"
        );
        assert_eq!(
            verdict(&[], "Bash", json!({ "command": command })),
            Verdict::Nothing,
            "with no shell rule cached, {command:?} asks nothing"
        );
    }
    // A reserved word quoted, or past a segment's first word, is a word.
    assert_eq!(
        shell_segments("'if' x; echo if then fi; \"time\" y; echo $HOME '$x' \"$'z'\"")
            .expect("tokenises"),
        words(&[
            &["if", "x"],
            &["echo", "if", "then", "fi"],
            &["time", "y"],
            &["echo", "$HOME", "$x", "$'z'"],
        ])
    );
}

#[test]
fn a_final_star_word_matches_zero_or_more_words_from_the_first_word_on() {
    let star = cached(
        1,
        "s",
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["git", "push", "*"], "effect": "deny"}),
    );
    let exact = cached(
        2,
        "e",
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["git", "push"], "effect": "deny"}),
    );
    let middle = cached(
        3,
        "m",
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["git", "*", "main"], "effect": "deny"}),
    );
    let globbed = cached(
        4,
        "g",
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["rm", "-*f*", "*"], "effect": "deny"}),
    );
    assert_eq!(
        shell(std::slice::from_ref(&star), "git push"),
        Some(Effect::Deny),
        "zero words"
    );
    assert_eq!(
        shell(std::slice::from_ref(&star), "git push a b c"),
        Some(Effect::Deny)
    );
    assert_eq!(
        shell(std::slice::from_ref(&star), "sudo git push"),
        None,
        "from the first word on"
    );
    assert_eq!(shell(std::slice::from_ref(&star), "git"), None);
    assert_eq!(
        shell(std::slice::from_ref(&exact), "git push"),
        Some(Effect::Deny)
    );
    assert_eq!(
        shell(std::slice::from_ref(&exact), "git push origin"),
        None,
        "no final star, no more words"
    );
    assert_eq!(
        shell(std::slice::from_ref(&middle), "git push main"),
        Some(Effect::Deny)
    );
    assert_eq!(
        shell(std::slice::from_ref(&middle), "git main"),
        None,
        "an inner star is one word"
    );
    assert_eq!(
        shell(std::slice::from_ref(&middle), "git push origin main"),
        None
    );
    assert_eq!(
        shell(std::slice::from_ref(&globbed), "rm -rf /"),
        Some(Effect::Deny)
    );
    assert_eq!(
        shell(std::slice::from_ref(&globbed), "rm -f"),
        Some(Effect::Deny)
    );
    assert_eq!(shell(std::slice::from_ref(&globbed), "rm -r x"), None);
    // A word glob's star crosses a slash: words are not paths.
    let slash = cached(
        5,
        "p",
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["cat", "*.env"], "effect": "deny"}),
    );
    assert_eq!(shell(&[slash], "cat config/prod.env"), Some(Effect::Deny));
}

#[test]
fn a_path_and_its_glob_are_normalised_against_the_input_cwd() {
    assert_eq!(
        normalise_path("src/../a//b/./c/", Some("/work/repo")),
        Some("/work/repo/a/b/c".to_owned())
    );
    assert_eq!(
        normalise_path("/x/../../y", Some("/ignored")),
        Some("/y".to_owned())
    );
    assert_eq!(normalise_path("/", None), Some("/".to_owned()));
    assert_eq!(
        normalise_path("a", None),
        None,
        "a relative path needs a cwd"
    );
    assert_eq!(
        normalise_path("a", Some("relative")),
        None,
        "and the cwd must be absolute"
    );
    assert_eq!(
        normalise_path("", Some("/w")),
        None,
        "an empty path is no path"
    );

    let relative_glob = [cached(
        1,
        "no secrets",
        json!({"rule": "oml-gr/1", "tool": "edit", "path": "secrets/*", "effect": "deny"}),
    )];
    for path in [
        "secrets/k",
        "/work/repo/secrets/k",
        "./x/../secrets//k",
        "/work/repo/a/../secrets/k",
    ] {
        assert_eq!(
            effect(&verdict(
                &relative_glob,
                "Write",
                json!({ "file_path": path })
            )),
            Some(Effect::Deny),
            "{path}"
        );
    }
    assert_eq!(
        effect(&verdict(
            &relative_glob,
            "Write",
            json!({"file_path": "/elsewhere/secrets/k"})
        )),
        None
    );
    let dotted_glob = [cached(
        1,
        "g",
        json!({"rule": "oml-gr/1", "tool": "read", "path": "/work/./repo//x/../*.pem", "effect": "deny"}),
    )];
    assert_eq!(
        effect(&verdict(
            &dotted_glob,
            "Read",
            json!({"file_path": "k.pem"})
        )),
        Some(Effect::Deny)
    );
}

#[test]
fn a_star_stops_at_a_slash_and_a_double_star_does_not() {
    let star = Glob::path("/a/*.rs");
    assert!(star.matches("/a/x.rs"));
    assert!(star.matches("/a/.rs"), "a star matches a leading dot");
    assert!(!star.matches("/a/b/x.rs"));
    let deep = Glob::path("/a/**.rs");
    assert!(deep.matches("/a/x.rs"));
    assert!(deep.matches("/a/b/c/x.rs"));
    assert!(!deep.matches("/b/x.rs"));
    let question = Glob::path("/a/?");
    assert!(question.matches("/a/x"));
    assert!(
        question.matches("/a/."),
        "a question mark matches a leading dot"
    );
    assert!(
        question.matches("/a/\u{65E5}"),
        "one character, not one byte"
    );
    assert!(!question.matches("/a//"));
    assert!(!question.matches("/a/xy"));
    // Every other character is literal: no classes, no escapes.
    assert!(Glob::path("/a/[b]").matches("/a/[b]"));
    assert!(!Glob::path("/a/[b]").matches("/a/b"));
    // A word glob has no slash rule.
    assert!(Glob::word("*").matches("a/b"));
    assert!(Glob::word("?").matches("/"));
}

#[test]
fn a_fetch_host_is_the_whatwg_host_lower_cased_without_a_trailing_dot() {
    for (url, host) in [
        ("https://Example.COM/x", "example.com"),
        ("https://example.com./x", "example.com"),
        ("https://user:pw@example.com:8443/p?q#f", "example.com"),
        ("https://a@b@example.com/", "example.com"),
        ("HTTP://EXAMPLE.com", "example.com"),
        ("https:example.com/x", "example.com"),
        ("https:\\\\example.com\\x", "example.com"),
        ("  https://example.com/x \t", "example.com"),
        ("https://exa\tmple.com/", "example.com"),
        ("https://%65xample.com/", "example.com"),
        ("http://0x7f.1/", "127.0.0.1"),
        ("http://2130706433/", "127.0.0.1"),
        ("http://127.0.0.1./", "127.0.0.1"),
        ("http://0177.0.0.1/", "127.0.0.1"),
        ("http://[::1]:80/", "[::1]"),
        ("ssh://Git.Example.org/repo", "git.example.org"),
    ] {
        assert_eq!(url_host(url).as_deref(), Some(host), "{url}");
    }
    for url in [
        "",
        "example.com",
        "/relative",
        "mailto:a@example.com",
        "file:///etc/passwd",
        "https://",
        "https://exa mple.com/",
        "http://256.256.256.256/",
        "https://b\u{fc}cher.example/",
    ] {
        assert_eq!(url_host(url), None, "{url:?}");
    }

    let rules = [
        cached(
            1,
            "x",
            json!({"rule": "oml-gr/1", "tool": "fetch", "host": "example.com", "effect": "deny"}),
        ),
        cached(
            2,
            "y",
            json!({"rule": "oml-gr/1", "tool": "fetch", "host": "*.internal.example", "effect": "ask"}),
        ),
        cached(
            3,
            "z",
            json!({"rule": "oml-gr/1", "tool": "fetch", "host": "127.0.0.1", "effect": "deny"}),
        ),
    ];
    let fetch = |url: &str| effect(&verdict(&rules, "WebFetch", json!({ "url": url })));
    assert_eq!(fetch("https://EXAMPLE.com./a"), Some(Effect::Deny));
    assert_eq!(fetch("https://www.example.com/"), None, "exact means exact");
    assert_eq!(fetch("https://a.internal.example/"), Some(Effect::Ask));
    assert_eq!(fetch("https://a.b.Internal.Example/"), Some(Effect::Ask));
    assert_eq!(
        fetch("https://internal.example/"),
        None,
        "a suffix needs a label before it"
    );
    assert_eq!(fetch("https://evilinternal.example/"), None);
    assert_eq!(fetch("http://0x7f000001/"), Some(Effect::Deny));
    // A host the matcher cannot spell (here an IDN) asks.
    assert_eq!(fetch("https://b\u{fc}cher.example/"), Some(Effect::Ask));
}

#[test]
fn a_fetch_ipv6_host_is_compared_in_its_whatwg_serialisation() {
    for (url, host) in [
        ("http://[0:0:0:0:0:0:0:1]/", "[::1]"),
        ("http://[0::1]/", "[::1]"),
        ("http://[::0001]/", "[::1]"),
        ("http://[::]/", "[::]"),
        ("http://[1::]/", "[1::]"),
        ("http://[1:0:0:0:0:0:0:0]/", "[1::]"),
        ("http://[2001:DB8:0:0:1:0:0:1]:8080/", "[2001:db8::1:0:0:1]"),
        ("http://[1:0:2:3:4:5:6:7]/", "[1:0:2:3:4:5:6:7]"),
        ("http://[1:0:0:2:0:0:0:3]/", "[1:0:0:2::3]"),
        ("http://[::FFFF:127.0.0.1]/", "[::ffff:7f00:1]"),
        ("http://[::ffff:7f00:1]/", "[::ffff:7f00:1]"),
        ("ssh://[0:0::1]/repo", "[::1]"),
    ] {
        assert_eq!(url_host(url).as_deref(), Some(host), "{url}");
    }
    for url in [
        "http://[]/",
        "http://[:1]/",
        "http://[1:::2]/",
        "http://[::1::]/",
        "http://[1:2:3:4:5:6:7:8:9]/",
        "http://[1:2:3:4:5:6:7]/",
        "http://[12345::]/",
        "http://[::01.2.3.4]/",
        "http://[::1.2.3]/",
        "http://[::256.0.0.1]/",
        "http://[::g]/",
        "http://[::1]x/",
    ] {
        assert_eq!(url_host(url), None, "{url:?}");
    }
    let rules = [cached(
        1,
        "x",
        json!({"rule": "oml-gr/1", "tool": "fetch", "host": "[::1]", "effect": "deny"}),
    )];
    let fetch = |url: &str| effect(&verdict(&rules, "WebFetch", json!({ "url": url })));
    for url in [
        "http://[::1]/",
        "http://[0:0:0:0:0:0:0:1]/",
        "http://[0::1]/",
        "http://[::0001]/",
    ] {
        assert_eq!(fetch(url), Some(Effect::Deny), "{url}");
    }
    assert_eq!(fetch("http://[::2]/"), None);
}

/// A plain backtracking matcher, the reference the linear one must agree with.
fn reference(pattern: &[char], input: &[char], path: bool) -> bool {
    match pattern.first() {
        None => input.is_empty(),
        Some('*') => {
            let double = path && pattern.get(1) == Some(&'*');
            let rest = if double {
                let mut i = 0;
                while pattern.get(i) == Some(&'*') {
                    i += 1;
                }
                &pattern[i..]
            } else {
                &pattern[1..]
            };
            (0..=input.len()).any(|k| {
                (double || !path || !input[..k].contains(&'/'))
                    && reference(rest, &input[k..], path)
            })
        }
        Some('?') => match input.first() {
            Some(c) if !(path && *c == '/') => reference(&pattern[1..], &input[1..], path),
            _ => false,
        },
        Some(c) => input.first() == Some(c) && reference(&pattern[1..], &input[1..], path),
    }
}

#[test]
fn a_glob_of_many_stars_matches_a_long_command_in_linear_time() {
    // The classic blow-up for a backtracking matcher: (a*)ⁿb against aᵐ.
    let pattern = format!("{}b", "a*".repeat(64));
    let input = "a".repeat(100_000);
    for glob in [Glob::word(&pattern), Glob::path(&pattern)] {
        let (matched, steps) = glob.matches_counting(&input);
        assert!(!matched);
        assert!(steps <= (pattern.chars().count() as u64 + 1) * (input.len() as u64 + 1));
    }
    let rules = [cached(
        1,
        "s",
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["*a*a*a*a*a*a*a*a*a*a*b", "*"], "effect": "deny"}),
    )];
    assert_eq!(shell(&rules, &format!("{} x", "a".repeat(50_000))), None);
    assert_eq!(
        shell(&rules, &format!("{}b x", "a".repeat(50_000))),
        Some(Effect::Deny)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2_000))]

    #[test]
    fn the_glob_agrees_with_backtracking_on_adversarial_patterns_within_the_linear_bound(
        pattern in proptest::collection::vec(prop_oneof![Just('*'), Just('*'), Just('?'), Just('a'), Just('b'), Just('/')], 0..12),
        input in proptest::collection::vec(prop_oneof![Just('a'), Just('a'), Just('b'), Just('/')], 0..14),
    ) {
        let p: String = pattern.iter().collect();
        let s: String = input.iter().collect();
        for (glob, path) in [(Glob::word(&p), false), (Glob::path(&p), true)] {
            let (matched, steps) = glob.matches_counting(&s);
            prop_assert_eq!(matched, reference(&pattern, &input, path), "{:?} on {:?}, path {}", p, s, path);
            prop_assert_eq!(matched, glob.matches(&s));
            prop_assert!(steps <= (pattern.len() as u64 + 1) * (input.len() as u64 + 1));
        }
    }

    #[test]
    fn no_input_ever_answers_allow(
        tool in proptest::sample::select(vec!["Bash", "PowerShell", "Write", "Edit", "Read", "Grep", "Glob", "WebFetch", "mcp__a__b", "WebSearch", "Agent"]),
        text in "[ -~]{0,40}",
    ) {
        // Effect has no allow; with no rule cached nothing decides at all,
        // and with only ask rules nothing denies.
        prop_assert_eq!(Effect::parse("allow"), None);
        let input = json!({"command": text, "file_path": text, "path": text, "url": text});
        prop_assert_eq!(verdict(&[], tool, input.clone()), Verdict::Nothing);
        let asks = [cached(9, "a", json!({"rule": "oml-gr/1", "tool": "shell", "command": ["*"], "effect": "ask"}))];
        prop_assert_ne!(effect(&verdict(&asks, tool, input)), Some(Effect::Deny));
    }
}

// --- d4, d8: deny wins over ask, and the strictest wins -------------------

#[test]
fn a_matching_deny_rule_answers_deny_with_the_guardrail_id_and_body() {
    let v = verdict(&[deny_push()], "Bash", json!({"command": "git push"}));
    let Verdict::Decide(decision) = v else {
        panic!("{v:?}")
    };
    assert_eq!(decision.effect, Effect::Deny);
    assert_eq!(
        decision.causes,
        vec![Cause::Rule {
            artifact_id: Uuid::from_u128(1),
            body: "Never push from an agent session.".to_owned()
        }]
    );
    let reason = decision.reason();
    assert!(reason.contains(&Uuid::from_u128(1).to_string()), "{reason}");
    assert!(
        reason.contains("Never push from an agent session."),
        "{reason}"
    );
    // No rule matches: nothing is printed.
    assert_eq!(
        verdict(&[deny_push()], "Bash", json!({"command": "git status"})),
        Verdict::Nothing
    );
}

#[test]
fn deny_wins_over_ask() {
    let ask = cached(
        2,
        "Ask before any git command.",
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["git", "*"], "effect": "ask"}),
    );
    let deny = deny_push();
    for rules in [
        vec![ask.clone(), deny.clone()],
        vec![deny.clone(), ask.clone()],
    ] {
        let Verdict::Decide(d) =
            verdict(&rules, "Bash", json!({"command": "git status; git push"}))
        else {
            panic!()
        };
        assert_eq!(d.effect, Effect::Deny);
        assert_eq!(
            d.causes,
            vec![Cause::Rule {
                artifact_id: deny.artifact_id,
                body: deny.body.clone()
            }],
            "only the winning effect is named"
        );
    }
    let Verdict::Decide(d) = verdict(
        &[ask.clone(), deny],
        "Bash",
        json!({"command": "git status"}),
    ) else {
        panic!()
    };
    assert_eq!(d.effect, Effect::Ask);
    assert_eq!(
        Effect::Deny.max(Effect::Ask),
        Effect::Deny,
        "deny is the stricter effect"
    );
    // A command that does not tokenise has no segments to deny, so it asks.
    assert_eq!(shell(&[ask], "git push $(x)"), Some(Effect::Ask));
}

#[test]
fn an_edit_of_the_cache_directory_or_a_shell_word_naming_it_asks() {
    // The built-in rule needs no cached rule at all.
    for (tool, input) in [
        ("Write", json!({"file_path": format!("{CACHE_DIR}/s.json")})),
        (
            "Edit",
            json!({"file_path": format!("{CACHE_DIR}/../guardrails/s.json")}),
        ),
        ("Edit", json!({"file_path": CACHE_DIR})),
        (
            "Bash",
            json!({"command": "rm ~/.local/state/oml/guardrails/s.json"}),
        ),
        (
            "Bash",
            json!({"command": "echo x > a; cat 'oml/guardrails'"}),
        ),
        ("Bash", json!({"command": "cat $(ls oml/guardrails)"})),
    ] {
        let v = verdict(&[], tool, input.clone());
        assert_eq!(
            v,
            Verdict::Decide(oml_guardrail::Decision {
                effect: Effect::Ask,
                causes: vec![Cause::CacheDirectory]
            }),
            "{tool} {input}"
        );
    }
    for (tool, input) in [
        (
            "Write",
            json!({"file_path": format!("{CACHE_DIR}-other/s.json")}),
        ),
        ("Read", json!({"file_path": format!("{CACHE_DIR}/s.json")})),
        ("Bash", json!({"command": "ls ~/.local/state/oml"})),
    ] {
        assert_eq!(
            verdict(&[], tool, input.clone()),
            Verdict::Nothing,
            "{tool} {input}"
        );
    }
    // A deny still wins over the built-in ask.
    let deny_rm = cached(
        7,
        "no rm",
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["rm", "*"], "effect": "deny"}),
    );
    assert_eq!(shell(&[deny_rm], "rm oml/guardrails/x"), Some(Effect::Deny));
}

#[test]
fn a_redirection_into_the_cache_directory_asks() {
    for command in [
        "echo x > ~/.local/state/oml/guardrails/s.json",
        "echo x >~/.local/state/oml/guardrails/s.json",
        "echo x >> ~/.local/state/oml/guardrails/s.json",
        "echo x &> ~/.local/state/oml/guardrails/s.json",
        "echo x &>> ~/.local/state/oml/guardrails/s.json",
        "echo x 2> ~/.local/state/oml/guardrails/s.json",
        "echo x >| ~/.local/state/oml/guardrails/s.json",
        "> ~/.local/state/oml/guardrails/s.json",
        "make; echo x > 'oml/guardrails/s.json'",
    ] {
        assert_eq!(
            verdict(&[], "Bash", json!({ "command": command })),
            Verdict::Decide(oml_guardrail::Decision {
                effect: Effect::Ask,
                causes: vec![Cause::CacheDirectory]
            }),
            "{command:?}"
        );
    }
    // A redirection's target is still no word a rule matches on.
    let any = cached(
        2,
        "deny every command",
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["*"], "effect": "deny"}),
    );
    assert_eq!(shell(&[deny_push()], "echo x > git push"), None);
    assert_eq!(shell(std::slice::from_ref(&any), "> out"), None);
    assert_eq!(shell(&[any], "echo x > out"), Some(Effect::Deny));
}

// --- d9: the cache file codec and its failure modes ----------------------

fn sample_rules() -> Vec<CachedRule> {
    vec![
        deny_push(),
        cached(
            2,
            "Ask before \"fetching\"\ninternal hosts.",
            json!({"rule": "oml-gr/1", "tool": "fetch", "host": "*.internal.example", "effect": "ask"}),
        ),
    ]
}

#[test]
fn the_cache_is_one_json_line_followed_by_its_sha_256() {
    let text = encode_cache(&sample_rules());
    let lines: Vec<&str> = text.split_terminator('\n').collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(text.ends_with('\n'));
    assert_eq!(lines[1], hex::encode(Sha256::digest(lines[0].as_bytes())));
    let line: Value = serde_json::from_str(lines[0]).expect("one JSON line");
    assert_eq!(
        line["rules"][0]["artifact_id"],
        json!(Uuid::from_u128(1).to_string())
    );
    assert_eq!(
        line["rules"][1]["body"],
        json!("Ask before \"fetching\"\ninternal hosts.")
    );
    assert_eq!(line["rules"][0]["rule"]["rule"], json!("oml-gr/1"));
    assert_eq!(
        decode_cache(Some(text.as_bytes())),
        CacheState::Loaded(sample_rules())
    );
    assert_eq!(
        decode_cache(Some(encode_cache(&[]).as_bytes())),
        CacheState::Loaded(vec![])
    );
}

fn denies_every_call(state: &CacheState, refusal: &CacheRefusal) {
    for (tool, input) in [
        ("Bash", json!({"command": "ls"})),
        ("Read", json!({"file_path": "/a"})),
        ("WebFetch", json!({"url": "https://a.example"})),
        ("mcp__a__b", json!({})),
    ] {
        let v = decide(
            state,
            &ToolCall {
                tool_name: tool,
                tool_input: &input,
                cwd: Some(CWD),
            },
            CACHE_DIR,
        );
        let Verdict::Decide(d) = v else {
            panic!("{tool}: {v:?}")
        };
        assert_eq!(d.effect, Effect::Deny, "{tool}");
        assert_eq!(d.causes, vec![Cause::Cache(refusal.clone())]);
        let reason = d.reason();
        assert!(
            reason.contains("restart the session"),
            "names the fix: {reason}"
        );
    }
}

#[test]
fn an_unreadable_cache_denies_every_call() {
    let good = encode_cache(&sample_rules());
    let (line, _) = good.split_once('\n').expect("two lines");
    let digest = |s: &str| hex::encode(Sha256::digest(s.as_bytes()));
    let with_digest = |s: &str| format!("{s}\n{}\n", digest(s));
    for contents in [
        Vec::new(),
        b"\xff\xfe".to_vec(),
        line.as_bytes().to_vec(),
        format!("{line}\n").into_bytes(),
        format!("{line}\n{}", digest(line)).into_bytes(),
        format!("{good}\n").into_bytes(),
        with_digest("not json").into_bytes(),
        with_digest("[]").into_bytes(),
        with_digest(r#"{"rules":[{"artifact_id":"x","body":"b","rule":{}}]}"#).into_bytes(),
        with_digest(r#"{"rules":[],"extra":1}"#).into_bytes(),
        with_digest(&format!(
            r#"{{"rules":[{{"artifact_id":"{}","body":"b","rule":{{"rule":"oml-gr/1","tool":"shell","effect":"deny"}}}}]}}"#,
            Uuid::from_u128(1)
        ))
        .into_bytes(),
    ] {
        let state = decode_cache(Some(contents.as_slice()));
        assert_eq!(state, CacheState::Broken(CacheRefusal::Unparsable), "{:?}", String::from_utf8_lossy(&contents));
        denies_every_call(&state, &CacheRefusal::Unparsable);
    }
}

#[test]
fn a_cache_whose_digest_line_does_not_match_denies_every_call() {
    let good = encode_cache(&sample_rules());
    let (line, digest) = good.trim_end().split_once('\n').expect("two lines");
    let tampered = line.replace("deny", "ask");
    for contents in [
        format!("{tampered}\n{digest}\n"),
        format!("{line}\n{}\n", digest.to_uppercase()),
        format!("{line}\n{}\n", &digest[1..]),
        format!("{line}\nnot-hex\n"),
    ] {
        let state = decode_cache(Some(contents.as_bytes()));
        assert_eq!(
            state,
            CacheState::Broken(CacheRefusal::DigestMismatch),
            "{contents}"
        );
        denies_every_call(&state, &CacheRefusal::DigestMismatch);
    }
}

#[test]
fn a_cache_naming_an_unknown_grammar_denies_every_call() {
    let line = format!(
        r#"{{"rules":[{{"artifact_id":"{}","body":"b","rule":{{"rule":"oml-gr/2","tool":"shell","command":["x"],"effect":"deny"}}}}]}}"#,
        Uuid::from_u128(1)
    );
    let contents = format!("{line}\n{}\n", hex::encode(Sha256::digest(line.as_bytes())));
    let state = decode_cache(Some(contents.as_bytes()));
    let refusal = CacheRefusal::UnknownGrammar("oml-gr/2".to_owned());
    assert_eq!(state, CacheState::Broken(refusal.clone()));
    denies_every_call(&state, &refusal);
}

#[test]
fn a_session_with_no_cache_decides_nothing_and_warns_in_one_line() {
    let state = decode_cache(None);
    assert_eq!(state, CacheState::NoCache);
    assert_eq!(
        NO_CACHE_WARNING,
        "OML guardrails are not loaded for this session, so tool calls are not checked; restart the session to load them"
    );
    assert!(!NO_CACHE_WARNING.contains('\n'));
    for tool in [
        "Bash",
        "PowerShell",
        "Write",
        "Read",
        "WebFetch",
        "mcp__a__b",
    ] {
        let input = json!({"command": "git push", "file_path": format!("{CACHE_DIR}/x")});
        let v = decide(
            &state,
            &ToolCall {
                tool_name: tool,
                tool_input: &input,
                cwd: Some(CWD),
            },
            CACHE_DIR,
        );
        assert_eq!(
            v,
            Verdict::Warn(NO_CACHE_WARNING),
            "{tool}: it keeps no state, so it repeats"
        );
    }
    let input = json!({});
    let v = decide(
        &state,
        &ToolCall {
            tool_name: "WebSearch",
            tool_input: &input,
            cwd: None,
        },
        CACHE_DIR,
    );
    assert_eq!(v, Verdict::Nothing, "an unmatched tool is no matching call");
}

#[test]
fn a_session_id_outside_letters_digits_and_hyphens_names_no_cache_file() {
    assert_eq!(
        cache_file_name("abc-123-XYZ").as_deref(),
        Ok("abc-123-XYZ.json")
    );
    for id in [
        "",
        "../x",
        "a/b",
        "a.b",
        "a b",
        "a_b",
        "s\u{e9}ssion",
        "a\n",
    ] {
        assert!(cache_file_name(id).is_err(), "{id:?}");
    }
}

// ---------------------------------------------------------------------------
// The key's guardrail scope and the served set's digest (d4, d5)
// ---------------------------------------------------------------------------
