//! The `shell` class's argument: a `command` tokenised by POSIX quoting
//! without expansion, then split into segments at unquoted `;`, `&`, `&&`,
//! `||`, `|` and newlines (proposed ADR-0037 d8).
//!
//! What the record makes the check ask on, rather than tokenise, is a
//! [`ShellRefusal`]: a quote or escape left open, command or process
//! substitution, a subshell, a brace group or a here-document. This reading
//! adds the remaining compound forms of the same shape, which d8 neither
//! names as bypasses nor asks on: a segment whose first unquoted word is a
//! reserved word (`if`, `while`, `for`, `case`, `!`, `time`, ...), and
//! `$'…'` or `$"…"` quoting, which the tokeniser does not decode. A
//! redirection is not a word: its operator and an IO number before it are
//! dropped, and its target word is kept apart from the segment's words, so
//! no rule matches it but the built-in cache rule sees it (d9). A `#` that
//! begins a word comments out the rest of its line, as POSIX tokenising
//! does.

/// Why a command was not tokenised. Whenever a `shell` rule is cached the
/// check asks on it (proposed ADR-0037 d8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum ShellRefusal {
    #[error("a quote is not closed")]
    UnterminatedQuote,
    #[error("the command ends in an escape")]
    TrailingEscape,
    #[error("it holds command substitution")]
    CommandSubstitution,
    #[error("it holds process substitution")]
    ProcessSubstitution,
    #[error("it holds a subshell")]
    Subshell,
    #[error("it holds a brace group")]
    BraceGroup,
    #[error("it holds a here-document")]
    HereDocument,
    #[error("a redirection has no target")]
    MissingRedirectTarget,
    /// A segment's first word is an unquoted reserved word that opens or
    /// continues a compound command or prefixes a pipeline: `if`, `then`,
    /// `else`, `elif`, `fi`, `do`, `done`, `while`, `until`, `for`,
    /// `select`, `case`, `esac`, `!`, `time` or `coproc`. The command
    /// behind it is no segment's first word, so no rule could match it.
    #[error("it holds a compound command or a reserved word before a command")]
    CompoundCommand,
    /// `$'…'` ANSI-C quoting or `$"…"` locale quoting, which this
    /// tokeniser does not decode.
    #[error("it holds $'' or $\"\" quoting")]
    DollarQuote,
}

/// The unquoted words that, first in a segment, open or continue a
/// compound command or prefix a pipeline: POSIX's reserved words other than
/// `{`, `}` and `in`, and Bash's `select`, `time` and `coproc`.
const RESERVED: [&str; 16] = [
    "if", "then", "else", "elif", "fi", "do", "done", "while", "until", "for", "select", "case",
    "esac", "!", "time", "coproc",
];

/// One segment of a `shell` command: its words, which rules match, and the
/// target words of its redirections, which only the built-in cache rule
/// reads (proposed ADR-0037 d8, d9).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellSegment {
    pub words: Vec<String>,
    pub redirect_targets: Vec<String>,
}

#[derive(Default)]
struct Tokeniser {
    segments: Vec<ShellSegment>,
    words: Vec<String>,
    redirect_targets: Vec<String>,
    word: String,
    /// A word has begun, possibly empty (`''`).
    in_word: bool,
    /// Some part of the current word was quoted or escaped.
    quoted: bool,
    /// The next word is a redirection's target, not a word.
    redirect_target: bool,
}

impl Tokeniser {
    fn end_word(&mut self) -> Result<(), ShellRefusal> {
        if !self.in_word {
            return Ok(());
        }
        let word = std::mem::take(&mut self.word);
        let quoted = std::mem::take(&mut self.quoted);
        self.in_word = false;
        if self.redirect_target {
            self.redirect_target = false;
            self.redirect_targets.push(word);
        } else if !quoted && (word == "{" || word == "}") {
            return Err(ShellRefusal::BraceGroup);
        } else if !quoted && self.words.is_empty() && RESERVED.contains(&word.as_str()) {
            return Err(ShellRefusal::CompoundCommand);
        } else {
            self.words.push(word);
        }
        Ok(())
    }

    fn end_segment(&mut self) -> Result<(), ShellRefusal> {
        self.end_word()?;
        if self.redirect_target {
            return Err(ShellRefusal::MissingRedirectTarget);
        }
        if !self.words.is_empty() || !self.redirect_targets.is_empty() {
            self.segments.push(ShellSegment {
                words: std::mem::take(&mut self.words),
                redirect_targets: std::mem::take(&mut self.redirect_targets),
            });
        }
        Ok(())
    }

    fn push(&mut self, c: char) {
        self.word.push(c);
        self.in_word = true;
    }

    /// A redirection operator begins at the current character: an all-digit
    /// unquoted word right before it is its IO number, not a word.
    fn begin_redirect(&mut self) -> Result<(), ShellRefusal> {
        if self.in_word && !self.quoted && self.word.bytes().all(|b| b.is_ascii_digit()) {
            self.word.clear();
            self.in_word = false;
        } else {
            self.end_word()?;
        }
        if self.redirect_target {
            return Err(ShellRefusal::MissingRedirectTarget);
        }
        self.redirect_target = true;
        Ok(())
    }
}

/// The segments of a `shell` command, each the list of its words with
/// quotes removed and nothing expanded; segments with no word are left out.
pub fn shell_segments(command: &str) -> Result<Vec<Vec<String>>, ShellRefusal> {
    Ok(shell_command(command)?
        .into_iter()
        .filter(|s| !s.words.is_empty())
        .map(|s| s.words)
        .collect())
}

/// The segments of a `shell` command with their redirection targets, quotes
/// removed and nothing expanded; a segment with neither words nor targets
/// is left out.
pub fn shell_command(command: &str) -> Result<Vec<ShellSegment>, ShellRefusal> {
    let mut t = Tokeniser::default();
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '#' if !t.in_word => while chars.next_if(|&n| n != '\n').is_some() {},
            ' ' | '\t' => t.end_word()?,
            '\n' | ';' | '|' => t.end_segment()?,
            '&' => {
                if chars.next_if_eq(&'>').is_some() {
                    // `&>` and `&>>` redirect both streams.
                    t.end_word()?;
                    chars.next_if_eq(&'>');
                    if t.redirect_target {
                        return Err(ShellRefusal::MissingRedirectTarget);
                    }
                    t.redirect_target = true;
                } else {
                    t.end_segment()?;
                }
            }
            '<' | '>' => {
                if chars.peek() == Some(&'(') {
                    return Err(ShellRefusal::ProcessSubstitution);
                }
                if c == '<' && chars.peek() == Some(&'<') {
                    return Err(ShellRefusal::HereDocument);
                }
                t.begin_redirect()?;
                if c == '>' {
                    chars.next_if(|&n| matches!(n, '>' | '|' | '&'));
                } else {
                    chars.next_if(|&n| matches!(n, '&' | '>'));
                }
            }
            '(' | ')' => return Err(ShellRefusal::Subshell),
            '`' => return Err(ShellRefusal::CommandSubstitution),
            '$' if chars.peek() == Some(&'(') => return Err(ShellRefusal::CommandSubstitution),
            '$' if matches!(chars.peek(), Some('\'' | '"')) => {
                return Err(ShellRefusal::DollarQuote);
            }
            '\\' => match chars.next() {
                None => return Err(ShellRefusal::TrailingEscape),
                // A line continuation joins the lines and is no character.
                Some('\n') => {}
                Some(n) => {
                    t.push(n);
                    t.quoted = true;
                }
            },
            '\'' => {
                t.in_word = true;
                t.quoted = true;
                loop {
                    match chars.next() {
                        None => return Err(ShellRefusal::UnterminatedQuote),
                        Some('\'') => break,
                        Some(n) => t.word.push(n),
                    }
                }
            }
            '"' => {
                t.in_word = true;
                t.quoted = true;
                loop {
                    match chars.next() {
                        None => return Err(ShellRefusal::UnterminatedQuote),
                        Some('"') => break,
                        Some('`') => return Err(ShellRefusal::CommandSubstitution),
                        Some('$') if chars.peek() == Some(&'(') => {
                            return Err(ShellRefusal::CommandSubstitution);
                        }
                        Some('\\') => match chars.next() {
                            None => return Err(ShellRefusal::UnterminatedQuote),
                            Some('\n') => {}
                            Some(n @ ('$' | '`' | '"' | '\\')) => t.word.push(n),
                            Some(n) => {
                                t.word.push('\\');
                                t.word.push(n);
                            }
                        },
                        Some(n) => t.word.push(n),
                    }
                }
            }
            c => t.push(c),
        }
    }
    t.end_segment()?;
    Ok(t.segments)
}
