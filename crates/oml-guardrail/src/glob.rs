//! The `oml-gr/1` glob: `*` matches any run and `?` one character; in a
//! path glob `*` and `?` stop at `/` and `**` does not, and each matches a
//! leading `.`; every other character is literal (ADR-0037 d8).
//!
//! The matcher is a simulation of the glob's automaton over the set of live
//! pattern positions, one input character at a time: O(pattern × input)
//! time and O(pattern) space, with no regular expression and no
//! backtracking, so no pattern a rule can spell runs away.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    Literal(char),
    /// `?`: one character, never `/` in a path glob.
    One,
    /// `*`: any run, never crossing `/` in a path glob.
    Star,
    /// `**` (a run of two or more stars) in a path glob: any run, `/`
    /// included.
    DoubleStar,
}

/// A compiled glob. Every string is a glob: no character is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glob {
    tokens: Vec<Token>,
    path: bool,
}

impl Glob {
    /// A word or name glob: a `shell` command word or an `mcp` tool name,
    /// where `*` and `?` cross `/` like any other character.
    #[must_use]
    pub fn word(pattern: &str) -> Self {
        Self::compile(pattern, false)
    }

    /// A path glob, for `edit` and `read`: `*` and `?` stop at `/`, `**`
    /// does not.
    #[must_use]
    pub fn path(pattern: &str) -> Self {
        Self::compile(pattern, true)
    }

    fn compile(pattern: &str, path: bool) -> Self {
        let mut tokens: Vec<Token> = Vec::with_capacity(pattern.len());
        for c in pattern.chars() {
            let token = match c {
                '*' => match tokens.last_mut() {
                    // A run of stars is one star, or one double star in a
                    // path glob; collapsing changes no match.
                    Some(last @ (Token::Star | Token::DoubleStar)) => {
                        if path {
                            *last = Token::DoubleStar;
                        }
                        continue;
                    }
                    _ => Token::Star,
                },
                '?' => Token::One,
                c => Token::Literal(c),
            };
            tokens.push(token);
        }
        Glob { tokens, path }
    }

    /// Whether the whole input matches the whole glob.
    #[must_use]
    pub fn matches(&self, input: &str) -> bool {
        self.matches_counting(input).0
    }

    /// [`Glob::matches`], with the number of pattern positions the
    /// simulation examined, which is at most (pattern characters + 1) ×
    /// (input characters + 1). It is returned so that the bound is a
    /// tested property rather than a claim.
    #[must_use]
    pub fn matches_counting(&self, input: &str) -> (bool, u64) {
        let m = self.tokens.len();
        let mut live = vec![false; m + 1];
        let mut next = vec![false; m + 1];
        self.enable(&mut live, 0);
        let mut steps = 0_u64;
        for c in input.chars() {
            next.iter_mut().for_each(|s| *s = false);
            let mut any = false;
            let crosses = !(self.path && c == '/');
            for (p, (&token, &alive)) in self.tokens.iter().zip(&live).enumerate() {
                steps += 1;
                if !alive {
                    continue;
                }
                let to = match token {
                    Token::Literal(l) if l == c => p + 1,
                    Token::One if crosses => p + 1,
                    Token::Star if crosses => p,
                    Token::DoubleStar => p,
                    _ => continue,
                };
                self.enable(&mut next, to);
                any = true;
            }
            std::mem::swap(&mut live, &mut next);
            if !any {
                return (false, steps);
            }
        }
        (live[m], steps)
    }

    /// Marks position `q` live, and the position after it when `q` is a
    /// star, which may match nothing. Runs of stars are collapsed at
    /// compile time, so a star is never followed by a star and one step
    /// closes the set.
    fn enable(&self, live: &mut [bool], q: usize) {
        live[q] = true;
        if matches!(self.tokens.get(q), Some(Token::Star | Token::DoubleStar)) {
            live[q + 1] = true;
        }
    }
}
