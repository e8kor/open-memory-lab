//! The `shell` class's argument: a `command` tokenised by POSIX quoting
//! without expansion, then split into segments at unquoted `;`, `&`, `&&`,
//! `||`, `|` and newlines (proposed ADR-0037 d8).
//!
//! What the record makes the check ask on, rather than tokenise, is a
//! [`ShellRefusal`]: a quote or escape left open, command or process
//! substitution, a subshell, a brace group or a here-document. A
//! redirection is not a word: its operator, an IO number before it and its
//! target word are dropped from the segment, and a `#` that begins a word
//! comments out the rest of its line, as POSIX tokenising does.

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
}

#[derive(Default)]
struct Tokeniser {
    segments: Vec<Vec<String>>,
    words: Vec<String>,
    word: String,
    /// A word has begun, possibly empty (`''`).
    in_word: bool,
    /// Some part of the current word was quoted or escaped.
    quoted: bool,
    /// The next word is a redirection's target and is dropped.
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
        } else if !quoted && (word == "{" || word == "}") {
            return Err(ShellRefusal::BraceGroup);
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
        if !self.words.is_empty() {
            self.segments.push(std::mem::take(&mut self.words));
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
/// quotes removed and nothing expanded; empty segments are left out.
pub fn shell_segments(command: &str) -> Result<Vec<Vec<String>>, ShellRefusal> {
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
