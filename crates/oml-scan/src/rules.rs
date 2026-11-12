//! The `oml-scan/1.2` rule table (ADR-0065 d2–d5). Each rule names its
//! finding class, its action and one pattern; a rule whose match needs a
//! second look (an entropy floor, a placeholder password) names that check.
//!
//! The provider-token patterns marked `gitleaks` are adapted from the rule
//! regexes of gitleaks' default configuration (`config/gitleaks.toml`,
//! <https://github.com/gitleaks/gitleaks>), MIT licence, Copyright (c) 2019
//! Zachary Rice. Only the regexes are adapted, rewritten for Rust's `regex`
//! (no look-around); no gitleaks code is used (ADR-0065 d3). The full MIT
//! notice is `LICENSES/gitleaks-MIT.txt`, shipped in every release archive
//! and image.
//!
//! Word boundaries are ASCII (`(?-u:\b)`): a Unicode boundary makes the
//! matcher leave its DFA for a slower engine on any non-ASCII text (d9).

/// What a finding does to the submission (ADR-0001 R4; admission.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    /// Refuse the write: `submission.scan_veto`, nothing journalled.
    Veto,
    /// Admit it held for review, the finding recorded as the reason.
    Hold,
}

impl Action {
    /// The label the metrics and the doctor line spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Veto => "veto",
            Self::Hold => "hold",
        }
    }
}

/// The second look a match needs before it is a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Check {
    /// The match is the finding.
    None,
    /// Capture group 1 is a value that must look generated: long enough,
    /// a letter and a digit, Shannon entropy over the floor, no placeholder.
    Entropy,
    /// Capture groups 1 and 2 are a user and a password; the password must
    /// not be a placeholder, the user's own name or an environment reference.
    Password,
}

/// One rule of the set.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Rule {
    pub id: &'static str,
    pub class: &'static str,
    pub action: Action,
    pub pattern: &'static str,
    pub check: Check,
    /// Off unless the deployment turns the PII rules on (ADR-0065 d6).
    pub pii: bool,
}

const fn veto(id: &'static str, class: &'static str, pattern: &'static str) -> Rule {
    Rule {
        id,
        class,
        action: Action::Veto,
        pattern,
        check: Check::None,
        pii: false,
    }
}

const fn hold(id: &'static str, class: &'static str, pattern: &'static str) -> Rule {
    Rule {
        id,
        class,
        action: Action::Hold,
        pattern,
        check: Check::None,
        pii: false,
    }
}

const fn checked(rule: Rule, check: Check) -> Rule {
    Rule { check, ..rule }
}

const fn pii(rule: Rule) -> Rule {
    Rule { pii: true, ..rule }
}

/// The rules, in reporting order. Ids are stable within `oml-scan/1.2`; a
/// changed pattern or a new rule is a new name, `oml-scan/1.3` for a fix (ADR-0065 d1; ADR-0074 d21).
pub(crate) const RULES: &[Rule] = &[
    // ---- d3: secrets, veto -------------------------------------------------
    veto(
        "private-key-block",
        "secret.private_key",
        r"-----BEGIN[ A-Z0-9_-]{0,100}PRIVATE KEY(?: BLOCK)?-----",
    ),
    veto(
        "putty-private-key",
        "secret.private_key",
        r"PuTTY-User-Key-File-[0-9]+: ",
    ),
    // gitleaks aws-access-token
    veto(
        "aws-access-key-id",
        "secret.api_token",
        r"(?:^|[^A-Za-z0-9])(?:A3T[A-Z0-9]|AKIA|ASIA|ABIA|ACCA)[A-Z2-7]{16}(?-u:\b)",
    ),
    checked(
        veto(
            "aws-secret-access-key",
            "secret.assignment",
            r#"(?i)aws_?secret_?(?:access_?)?key["']?\s*(?:=|:=|:|=>)\s*["']?([A-Za-z0-9/+]{40})(?:[^A-Za-z0-9/+]|$)"#,
        ),
        Check::Entropy,
    ),
    // gitleaks github-pat, github-oauth, github-app-token, github-refresh-token
    veto(
        "github-token",
        "secret.api_token",
        r"gh[pousr]_[0-9A-Za-z]{36}(?-u:\b)",
    ),
    // gitleaks github-fine-grained-pat
    veto(
        "github-fine-grained-pat",
        "secret.api_token",
        r"github_pat_[0-9A-Za-z_]{82}(?-u:\b)",
    ),
    // gitleaks gitlab-pat, gitlab-ptt, gitlab-rrt, gitlab-deploy-token
    veto(
        "gitlab-token",
        "secret.api_token",
        r"gl(?:pat|ptt|rt|dt)-[0-9A-Za-z_-]{20,}",
    ),
    // gitleaks slack-bot-token, slack-user-token, slack-app-token
    veto(
        "slack-token",
        "secret.api_token",
        r"xox[abposr]-[0-9]{9,13}-[0-9A-Za-z-]{10,}",
    ),
    veto(
        "slack-app-token",
        "secret.api_token",
        r"xapp-[0-9]-[A-Z0-9]{9,}-[0-9]{9,}-[a-z0-9]{32,}",
    ),
    // gitleaks slack-webhook-url
    veto(
        "slack-webhook",
        "secret.api_token",
        r"hooks\.slack\.com/(?:services|workflows|triggers)/[A-Za-z0-9+/]{43,56}",
    ),
    // gitleaks stripe-access-token
    veto(
        "stripe-key",
        "secret.api_token",
        r"(?:sk|rk)_(?:test|live|prod)_[0-9A-Za-z]{10,99}(?-u:\b)",
    ),
    // gitleaks gcp-api-key
    veto(
        "google-api-key",
        "secret.api_token",
        r"AIza[0-9A-Za-z_-]{35}",
    ),
    // gitleaks openai-api-key, both shapes
    veto(
        "openai-key",
        "secret.api_token",
        r"sk-(?:(?:proj|svcacct|admin)-[A-Za-z0-9_-]{40,}|[A-Za-z0-9]{20}T3BlbkFJ[A-Za-z0-9]{20}|[A-Za-z0-9]{48}(?-u:\b))",
    ),
    // gitleaks anthropic-api-key, anthropic-admin-api-key
    veto(
        "anthropic-key",
        "secret.api_token",
        r"sk-ant-(?:api|admin|oat|ort)[0-9]{2}-[A-Za-z0-9_-]{80,}",
    ),
    // gitleaks npm-access-token, pypi-upload-token, huggingface-access-token
    veto(
        "npm-token",
        "secret.api_token",
        r"npm_[A-Za-z0-9]{36}(?-u:\b)",
    ),
    veto(
        "pypi-token",
        "secret.api_token",
        r"pypi-AgEIcHlwaS5vcmc[A-Za-z0-9_-]{50,}",
    ),
    veto(
        "huggingface-token",
        "secret.api_token",
        r"hf_[A-Za-z]{34}(?-u:\b)",
    ),
    // gitleaks jwt: three base64url parts, the first two JSON objects
    veto(
        "jwt",
        "secret.api_token",
        r"(?-u:\b)eyJ[A-Za-z0-9_-]{15,}\.eyJ[A-Za-z0-9_-]{15,}\.[A-Za-z0-9_-]{10,}",
    ),
    // OML's own credentials (roles-and-keys.md; ADR-0031 d10)
    veto(
        "oml-key",
        "secret.api_token",
        r"oml_[A-Za-z0-9]{43}(?-u:\b)",
    ),
    veto(
        "oml-connection-token",
        "secret.api_token",
        r"oml_(?:at|rt)_[A-Za-z0-9]{43}(?-u:\b)",
    ),
    checked(
        veto(
            "authorization-header",
            "secret.api_token",
            r#"(?i)authorization["']?\s*:\s*["']?(?:bearer|basic|token)\s+([A-Za-z0-9._~+/-]{16,}=*)"#,
        ),
        Check::Entropy,
    ),
    checked(
        veto(
            "url-credentials",
            "secret.credential_pair",
            r"(?-u:\b)[a-zA-Z][a-zA-Z0-9+.-]{1,30}://([^\s:/@?#]{1,128}):([^\s/@?#]{1,256})@[^\s/@?#]",
        ),
        Check::Password,
    ),
    checked(
        veto(
            "curl-user",
            "secret.credential_pair",
            r#"(?:^|\s)-u[ \t]*["']?([^\s:'"]{1,128}):([^\s'"]{1,256})"#,
        ),
        Check::Password,
    ),
    checked(
        veto(
            "keyword-assignment",
            "secret.assignment",
            r#"(?i)(?:^|[^A-Za-z0-9])(?:[A-Za-z0-9]+[_.-])*(?:password|passwd|pwd|secret|token|api[_-]?key|apikey|access[_-]?key|private[_-]?key|client[_-]?secret|credentials?)(?:[_.-][A-Za-z0-9]+){0,2}["']?[ \t]*(?:=|:=|:|=>)[ \t]*["'`]?([^\s"'`,;<>{}()\[\]|]{12,256})"#,
        ),
        Check::Entropy,
    ),
    // ---- compatibility: the PoC's test markers (ADR-0065 d10) --------------
    veto("marker-veto", "test.marker", r"\[oml:veto\]"),
    hold("marker-hold", "test.marker", r"\[oml:hold\]"),
    // ---- d5: hold for review -----------------------------------------------
    hold(
        "ignore-instructions",
        "injection.override",
        r"(?i)(?-u:\b)(?:ignore|disregard|forget|override)\s+(?:all\s+|any\s+|the\s+|of\s+)*(?:previous|prior|above|earlier|preceding|your|system|original)\s+(?:instructions?|prompts?|rules|directions|guidelines|directives)(?-u:\b)",
    ),
    hold(
        "new-instructions",
        "injection.override",
        r"(?i)(?-u:\b)(?:new|updated|real)\s+(?:system\s+)?instructions\s*:",
    ),
    hold(
        "chat-template",
        "injection.role",
        r"(?i)<\|im_start\|>|<\|(?:system|assistant|user)\|>|\[/?INST\]|<</?SYS>>",
    ),
    hold(
        "system-prompt-tag",
        "injection.role",
        r"(?i)<\s*/?\s*(?:system|system[_-]prompt|developer)\s*>|(?m)^\s*\[?(?:system|developer)\s+(?:prompt|message)\]?\s*:",
    ),
    hold(
        "persona-switch",
        "injection.role",
        r"(?i)(?-u:\b)you\s+are\s+now\s+(?:a|an|in)\s+(?:unrestricted|jailbroken|uncensored|new|different|developer|DAN)(?-u:\b)|(?-u:\b)your\s+new\s+(?:role|persona|instructions)\s+(?:is|are)(?-u:\b)|(?-u:\b)developer\s+mode\s+(?:enabled|on)(?-u:\b)",
    ),
    hold(
        "reader-trigger",
        "injection.tool_call",
        r"(?i)(?-u:\b)(?:when|if|after|once)\s+(?:an?\s+(?:ai|agent|assistant|model|llm)|you)\s+(?:reads?|recalls?|retrieves?|loads?|process(?:es)?)\s+this(?-u:\b)",
    ),
    hold(
        "covert-action",
        "injection.tool_call",
        r"(?i)(?-u:\b)(?:silently|secretly|quietly)\s+(?:call|run|execute|send|post|upload|exfiltrate|forward|curl|fetch)(?-u:\b)|(?-u:\b)without\s+(?:telling|informing|asking|notifying)\s+the\s+user(?-u:\b)|(?-u:\b)do\s+not\s+(?:tell|inform|mention\s+(?:this\s+)?to)\s+the\s+user(?-u:\b)",
    ),
    hold(
        "tool-call-markup",
        "injection.tool_call",
        r#"(?i)<\s*/?\s*(?:tool_call|function_call|function_calls|tool_use)(?-u:\b)|"tool_calls"\s*:"#,
    ),
    hold(
        "bidi-control",
        "unicode.bidi",
        r"[\x{202A}-\x{202E}\x{2066}-\x{2069}]",
    ),
    // ---- d6: personal data, off by default --------------------------------
    pii(hold(
        "email-address",
        "pii.email",
        r"(?-u:\b)[A-Za-z0-9._%+-]{1,64}@[A-Za-z0-9-]{1,63}(?:\.[A-Za-z0-9-]{1,63})*\.[A-Za-z]{2,24}(?-u:\b)",
    )),
    pii(hold(
        "phone-number",
        "pii.phone",
        r"(?:^|[^0-9A-Za-z+])(\+[1-9][0-9]{0,2}[ .-]?(?:\([0-9]{1,4}\)[ .-]?)?[0-9]{2,4}(?:[ .-]?[0-9]{2,4}){2,4}|\([0-9]{3}\)\s?[0-9]{3}-[0-9]{4})(?-u:\b)",
    )),
];

/// Values a key-value or URL rule never reports: documentation's stand-ins.
pub(crate) const PLACEHOLDERS: &[&str] = &[
    "password",
    "passwd",
    "pass",
    "secret",
    "changeme",
    "change-me",
    "example",
    "redacted",
    "placeholder",
    "token",
    "xxx",
    "***",
    "...",
    "your",
    "dummy",
    "sample",
];

/// What the text as written must show right after a letters-only phrase,
/// since the view dropped it with every other symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tail {
    /// Nothing more.
    None,
    /// A colon, with no letter or digit before it: the raw pattern's
    /// `\s*:`, and the separators a spelled-out phrase puts after its
    /// last letter.
    Colon,
}

/// The injection phrases the letters-only view reads (ADR-0074 d6; ADR-0065
/// d14 as amended 2026-11-10): each rule's phrase with every separator
/// removed, matched against the field's letters and digits alone, lower
/// case. Only phrases of three words or more, so ordinary words that run
/// together once spaces and punctuation go do not match; markup rules are
/// not here, since the view drops the symbols they name. Each pattern is
/// a set of literals written as alternations, with no word boundary: the
/// view has none. A match counts only where the text as written has a word
/// edge at each end of it, and shows the phrase's [`Tail`] (#226).
pub(crate) const LETTER_PHRASES: &[(&str, &str, Tail)] = &[
    (
        "ignore-instructions",
        "(?:ignore|disregard|forget|override)(?:all|any|the|of)*(?:previous|prior|above|earlier|preceding|your|system|original)(?:instructions?|prompts?|rules|directions|guidelines|directives)",
        Tail::None,
    ),
    (
        "new-instructions",
        "(?:new|updated|real)(?:system)?instructions",
        Tail::Colon,
    ),
    (
        "persona-switch",
        "youarenow(?:a|an|in)(?:unrestricted|jailbroken|uncensored|new|different|developer|dan)|yournew(?:role|persona|instructions)(?:is|are)|developermode(?:enabled|on)",
        Tail::None,
    ),
    (
        "reader-trigger",
        "(?:when|if|after|once)(?:an?(?:ai|agent|assistant|model|llm)|you)(?:reads?|recalls?|retrieves?|loads?|process(?:es)?)this",
        Tail::None,
    ),
    (
        "covert-action",
        "without(?:telling|informing|asking|notifying)theuser|donot(?:tell|inform|mention(?:this)?to)theuser",
        Tail::None,
    ),
];
