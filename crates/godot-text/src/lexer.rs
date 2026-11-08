use crate::error::ParseError;

/// Punctuation tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Punct {
    CurlyOpen,
    CurlyClose,
    BracketOpen,
    BracketClose,
    ParenOpen,
    ParenClose,
    Colon,
    Comma,
    Period,
    Equal,
}

impl Punct {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Punct::CurlyOpen => "'{'",
            Punct::CurlyClose => "'}'",
            Punct::BracketOpen => "'['",
            Punct::BracketClose => "']'",
            Punct::ParenOpen => "'('",
            Punct::ParenClose => "')'",
            Punct::Colon => "':'",
            Punct::Comma => "','",
            Punct::Period => "'.'",
            Punct::Equal => "'='",
        }
    }
}

/// Kind of a token. Borrowed variants point into the source text.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TokKind<'a> {
    Punct(Punct),
    Ident(&'a str),
    /// Raw number text, e.g. `-1.5e3`.
    Number(&'a str),
    /// A decoded `"..."` string.
    Str(String),
    /// A decoded `&"..."` string name.
    StrName(String),
    /// Raw colour literal including the `#`.
    Color(&'a str),
    Eof,
}

impl TokKind<'_> {
    pub(crate) fn describe(&self) -> String {
        match self {
            TokKind::Punct(p) => p.as_str().to_string(),
            TokKind::Ident(s) => format!("identifier '{s}'"),
            TokKind::Number(s) => format!("number '{s}'"),
            TokKind::Str(_) => "string".to_string(),
            TokKind::StrName(_) => "string name".to_string(),
            TokKind::Color(s) => format!("color '{s}'"),
            TokKind::Eof => "end of file".to_string(),
        }
    }
}

/// A token with its byte span in the source.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token<'a> {
    pub kind: TokKind<'a>,
    pub start: usize,
    pub end: usize,
}

/// Character-level cursor shared by the lexer and the document parser.
#[derive(Debug, Clone)]
pub(crate) struct Lexer<'a> {
    pub src: &'a str,
    pub pos: usize,
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(src: &'a str) -> Self {
        Lexer { src, pos: 0 }
    }

    pub(crate) fn peek(&self) -> Option<char> {
        self.src.get(self.pos..).and_then(|s| s.chars().next())
    }

    pub(crate) fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    pub(crate) fn slice(&self, start: usize, end: usize) -> &'a str {
        self.src.get(start..end).unwrap_or("")
    }

    pub(crate) fn error(&self, offset: usize, msg: impl Into<String>) -> ParseError {
        ParseError::at(self.src, offset, msg)
    }

    /// Skips whitespace and `;` comments.
    pub(crate) fn skip_trivia(&mut self) {
        while let Some(c) = self.peek() {
            if c == ';' {
                self.skip_comment();
            } else if (c as u32) <= 32 && c != '\0' {
                self.bump();
            } else {
                break;
            }
        }
    }

    /// Skips from a `;` to the end of the line (the newline is consumed too).
    pub(crate) fn skip_comment(&mut self) {
        while let Some(c) = self.bump() {
            if c == '\n' {
                break;
            }
        }
    }

    /// Returns the next token, skipping whitespace and comments.
    pub(crate) fn next_token(&mut self) -> Result<Token<'a>, ParseError> {
        self.skip_trivia();
        let start = self.pos;
        let Some(c) = self.bump() else {
            return Ok(Token {
                kind: TokKind::Eof,
                start,
                end: start,
            });
        };
        let punct = |p| Ok(TokKind::Punct(p));
        let kind: Result<TokKind<'a>, ParseError> = match c {
            '\0' => Err(self.error(start, "Unexpected NUL character")),
            '{' => punct(Punct::CurlyOpen),
            '}' => punct(Punct::CurlyClose),
            '[' => punct(Punct::BracketOpen),
            ']' => punct(Punct::BracketClose),
            '(' => punct(Punct::ParenOpen),
            ')' => punct(Punct::ParenClose),
            ':' => punct(Punct::Colon),
            ',' => punct(Punct::Comma),
            '.' => punct(Punct::Period),
            '=' => punct(Punct::Equal),
            '#' => {
                while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    self.bump();
                }
                Ok(TokKind::Color(self.slice(start, self.pos)))
            }
            '&' | '@' => {
                if self.peek() != Some('"') {
                    return Err(self.error(start, "Expected '\"' after '&'"));
                }
                self.bump();
                self.lex_string_body(start).map(TokKind::StrName)
            }
            '"' => self.lex_string_body(start).map(TokKind::Str),
            _ => self.lex_word(c, start),
        };
        Ok(Token {
            kind: kind?,
            start,
            end: self.pos,
        })
    }

    fn lex_word(&mut self, first: char, start: usize) -> Result<TokKind<'a>, ParseError> {
        let mut c = first;
        if c == '-' {
            match self.peek() {
                Some(n) => {
                    c = n;
                    self.bump();
                }
                None => return Err(self.error(start, "Unexpected character '-'")),
            }
        }
        if c.is_ascii_digit() {
            self.lex_number_tail();
            Ok(TokKind::Number(self.slice(start, self.pos)))
        } else if c.is_ascii_alphabetic() || c == '_' {
            while self
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                self.bump();
            }
            Ok(TokKind::Ident(self.slice(start, self.pos)))
        } else {
            Err(self.error(start, format!("Unexpected character {c:?}")))
        }
    }

    fn lex_number_tail(&mut self) {
        #[derive(PartialEq)]
        enum St {
            Int,
            Dec,
            Exp,
        }
        let mut st = St::Int;
        let mut exp_sign = false;
        let mut exp_beg = false;
        while let Some(c) = self.peek() {
            let accept = match st {
                St::Int => {
                    if c.is_ascii_digit() {
                        true
                    } else if c == '.' {
                        st = St::Dec;
                        true
                    } else if c == 'e' || c == 'E' {
                        st = St::Exp;
                        true
                    } else {
                        false
                    }
                }
                St::Dec => {
                    if c.is_ascii_digit() {
                        true
                    } else if c == 'e' || c == 'E' {
                        st = St::Exp;
                        true
                    } else {
                        false
                    }
                }
                St::Exp => {
                    if c.is_ascii_digit() {
                        exp_beg = true;
                        true
                    } else if (c == '-' || c == '+') && !exp_sign && !exp_beg {
                        exp_sign = true;
                        true
                    } else {
                        false
                    }
                }
            };
            if !accept {
                break;
            }
            self.bump();
        }
    }

    fn lex_string_body(&mut self, start: usize) -> Result<String, ParseError> {
        let mut out = String::new();
        let mut lead_surrogate: Option<u32> = None;
        loop {
            let at = self.pos;
            let Some(ch) = self.bump() else {
                return Err(self.error(start, "Unterminated string"));
            };
            match ch {
                '\0' => return Err(self.error(start, "Unterminated string")),
                '"' => break,
                '\\' => {
                    let Some(next) = self.bump() else {
                        return Err(self.error(start, "Unterminated string"));
                    };
                    let code: u32 = match next {
                        'b' => 8,
                        't' => 9,
                        'n' => 10,
                        'f' => 12,
                        'r' => 13,
                        'u' | 'U' => {
                            let len = if next == 'U' { 6 } else { 4 };
                            let mut v: u32 = 0;
                            for _ in 0..len {
                                let hex_at = self.pos;
                                let Some(h) = self.bump() else {
                                    return Err(self.error(start, "Unterminated string"));
                                };
                                let Some(d) = h.to_digit(16) else {
                                    return Err(
                                        self.error(hex_at, "Malformed hex constant in string")
                                    );
                                };
                                v = (v << 4) | d;
                            }
                            v
                        }
                        other => other as u32,
                    };
                    if (code & 0xffff_fc00) == 0xd800 {
                        if lead_surrogate.is_some() {
                            return Err(self.error(
                                at,
                                "Invalid UTF-16 sequence in string, unpaired lead surrogate",
                            ));
                        }
                        lead_surrogate = Some(code);
                        continue;
                    }
                    let code =
                        if (code & 0xffff_fc00) == 0xdc00 {
                            match lead_surrogate.take() {
                                Some(lead) => 0x10000 + ((lead - 0xd800) << 10) + (code - 0xdc00),
                                None => return Err(self.error(
                                    at,
                                    "Invalid UTF-16 sequence in string, unpaired trail surrogate",
                                )),
                            }
                        } else {
                            code
                        };
                    if lead_surrogate.is_some() {
                        return Err(self.error(
                            at,
                            "Invalid UTF-16 sequence in string, unpaired lead surrogate",
                        ));
                    }
                    match char::from_u32(code) {
                        Some(c) => out.push(c),
                        None => return Err(self.error(at, "Invalid Unicode code point in string")),
                    }
                }
                other => {
                    if lead_surrogate.is_some() {
                        return Err(self.error(
                            at,
                            "Invalid UTF-16 sequence in string, unpaired lead surrogate",
                        ));
                    }
                    out.push(other);
                }
            }
        }
        if lead_surrogate.is_some() {
            return Err(self.error(
                start,
                "Invalid UTF-16 sequence in string, unpaired lead surrogate",
            ));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokKind<'_>> {
        let mut lx = Lexer::new(src);
        let mut out = Vec::new();
        loop {
            let t = lx.next_token().unwrap();
            if t.kind == TokKind::Eof {
                break;
            }
            out.push(t.kind);
        }
        out
    }

    #[test]
    fn numbers_and_identifiers() {
        assert_eq!(
            kinds("1 -2.5 3e-4 -inf nan Vector2"),
            vec![
                TokKind::Number("1"),
                TokKind::Number("-2.5"),
                TokKind::Number("3e-4"),
                TokKind::Ident("-inf"),
                TokKind::Ident("nan"),
                TokKind::Ident("Vector2"),
            ]
        );
    }

    #[test]
    fn strings_and_escapes() {
        assert_eq!(
            kinds(r#""a\"b\n\u00e9\ud83d\ude00" &"name""#),
            vec![
                TokKind::Str("a\"b\né😀".to_string()),
                TokKind::StrName("name".to_string())
            ]
        );
        assert_eq!(
            kinds("\"x\\qy\nz\""),
            vec![TokKind::Str("xqy\nz".to_string())]
        );
    }

    #[test]
    fn comments_anywhere() {
        assert_eq!(
            kinds("1 ; comment ] [\n2"),
            vec![TokKind::Number("1"), TokKind::Number("2")]
        );
    }

    #[test]
    fn errors_have_positions() {
        let mut lx = Lexer::new("\n  \"abc");
        let e = lx.next_token().unwrap_err();
        assert_eq!((e.line, e.column), (2, 3));
        let mut lx = Lexer::new("$");
        assert!(lx.next_token().is_err());
        let mut lx = Lexer::new("\"\\ud800x\"");
        assert!(lx.next_token().is_err());
    }
}
