// Lexer for BitBake using logos
// Provides error-resilient tokenization

use crate::syntax_kind::SyntaxKind;
use logos::Logos;
use std::ops::Range;

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: SyntaxKind,
    pub text: String,
    pub span: Range<usize>,
}

pub struct Lexer<'a> {
    inner: logos::Lexer<'a, SyntaxKind>,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            inner: SyntaxKind::lexer(input),
        }
    }

    pub fn tokenize(input: &str) -> Vec<Token> {
        let mut inner = SyntaxKind::lexer(input);
        let mut tokens = Vec::new();

        while let Some(kind) = inner.next() {
            let kind = kind.unwrap_or(SyntaxKind::ERROR_TOKEN);
            let span = inner.span();
            let text = inner.slice().to_string();
            tokens.push(Token { kind, text, span });
        }

        // Always add EOF token
        let len = input.len();
        tokens.push(Token {
            kind: SyntaxKind::EOF,
            text: String::new(),
            span: len..len,
        });

        tokens
    }

    fn next_token(&mut self) -> Option<Token> {
        let kind = self.inner.next()?;
        let kind = kind.unwrap_or(SyntaxKind::ERROR_TOKEN);
        let span = self.inner.span();
        let text = self.inner.slice().to_string();

        Some(Token { kind, text, span })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_assignment() {
        let input = r#"FOO = "bar""#;
        let tokens = Lexer::tokenize(input);

        assert_eq!(tokens[0].kind, SyntaxKind::IDENT);
        assert_eq!(tokens[0].text, "FOO");

        assert_eq!(tokens[1].kind, SyntaxKind::WHITESPACE);

        assert_eq!(tokens[2].kind, SyntaxKind::EQ);

        assert_eq!(tokens[3].kind, SyntaxKind::WHITESPACE);

        assert_eq!(tokens[4].kind, SyntaxKind::STRING);
        assert_eq!(tokens[4].text, r#""bar""#);

        assert_eq!(tokens[5].kind, SyntaxKind::EOF);
    }

    #[test]
    fn test_append_operator() {
        let input = "FOO += \"value\"";
        let tokens = Lexer::tokenize(input);

        assert_eq!(tokens[2].kind, SyntaxKind::PLUS_EQ);
    }

    #[test]
    fn test_override_syntax() {
        let input = "FOO:append = \"value\"";
        let tokens = Lexer::tokenize(input);

        assert_eq!(tokens[0].kind, SyntaxKind::IDENT);
        assert_eq!(tokens[1].kind, SyntaxKind::COLON_APPEND);
        assert_eq!(tokens[3].kind, SyntaxKind::EQ);
    }

    #[test]
    fn test_variable_expansion() {
        // Variable expansion inside a string is part of the string token
        let input_in_string = r#"FOO = "${BAR}""#;
        let tokens = Lexer::tokenize(input_in_string);

        // The string token contains the variable expansion
        assert!(tokens.iter().any(|t| t.kind == SyntaxKind::STRING && t.text.contains("${BAR}")));

        // Variable expansion outside strings
        let input_standalone = "FOO = ${BAR}";
        let tokens2 = Lexer::tokenize(input_standalone);

        // Should have VAR_EXPANSION token when not in quotes
        assert!(tokens2.iter().any(|t| t.kind == SyntaxKind::VAR_EXPANSION));
    }

    #[test]
    fn test_comment() {
        let input = "# This is a comment\nFOO = \"bar\"";
        let tokens = Lexer::tokenize(input);

        assert_eq!(tokens[0].kind, SyntaxKind::COMMENT);
        assert_eq!(tokens[1].kind, SyntaxKind::NEWLINE);
        assert_eq!(tokens[2].kind, SyntaxKind::IDENT);
    }

    #[test]
    fn test_inherit_statement() {
        let input = "inherit cmake cargo";
        let tokens = Lexer::tokenize(input);

        assert_eq!(tokens[0].kind, SyntaxKind::INHERIT_KW);
        assert_eq!(tokens[2].kind, SyntaxKind::IDENT);
        assert_eq!(tokens[2].text, "cmake");
        assert_eq!(tokens[4].kind, SyntaxKind::IDENT);
        assert_eq!(tokens[4].text, "cargo");
    }

    #[test]
    fn test_multiline_value() {
        let input = r#"SRC_URI = "git://example.com/repo.git \
           file://patch.patch \
           ""#;
        let tokens = Lexer::tokenize(input);

        // Should tokenize with BACKSLASH and NEWLINE
        assert!(tokens.iter().any(|t| t.kind == SyntaxKind::BACKSLASH));
        assert!(tokens.iter().any(|t| t.kind == SyntaxKind::NEWLINE));
    }

    #[test]
    fn test_error_recovery() {
        let input = "FOO = @#$%^& \"bar\"";
        let tokens = Lexer::tokenize(input);

        // Should produce tokens even with invalid input
        assert_eq!(tokens[0].kind, SyntaxKind::IDENT);
        assert_eq!(tokens[0].text, "FOO");

        // logos 0.14+ converts unrecognized chars to ERROR_TOKEN via unwrap_or
        // The lexer should continue and find valid tokens after errors

        // Should still find the assignment operator
        assert!(tokens.iter().any(|t| t.kind == SyntaxKind::EQ));

        // The main test is that lexing doesn't crash and continues past errors
        // Verifying EOF token exists shows we completed lexing
        assert!(tokens.iter().any(|t| t.kind == SyntaxKind::EOF));
    }
}
