//! Lexer for query language using logos
//!
//! Provides proper tokenization with source locations for error reporting.

use logos::Logos;
use std::fmt;
use std::ops::Range;

/// Token types for the query language
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
#[logos(skip r"[ \t\r\n]+")]  // Skip whitespace
pub enum TokenKind {
    // Keywords (function names)
    #[token("deps")]
    Deps,
    #[token("rdeps")]
    Rdeps,
    #[token("somepath")]
    Somepath,
    #[token("allpaths")]
    Allpaths,
    #[token("kind")]
    Kind,
    #[token("filter")]
    Filter,
    #[token("attr")]
    Attr,
    #[token("script")]
    Script,
    #[token("inputs")]
    Inputs,
    #[token("outputs")]
    Outputs,
    #[token("env")]
    Env,
    #[token("critical-path")]
    CriticalPath,

    // Set operations
    #[token("intersect")]
    Intersect,
    #[token("union")]
    Union,
    #[token("except")]
    Except,

    // Special patterns
    #[token("//...")]
    AllTargets,

    #[token("...")]
    Ellipsis,

    // Punctuation
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token("*")]
    Star,

    // Literals
    #[regex(r#""([^"\\]|\\.)*""#)]
    DoubleQuotedString,

    #[regex(r#"'([^'\\]|\\.)*'"#)]
    SingleQuotedString,

    #[regex(r"[0-9]+")]
    Number,

    // Identifiers (recipe names, layer names, task names)
    // Allow alphanumeric, underscore, hyphen, dot
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_\-\.]*")]
    Ident,

    // Error token for unrecognized characters
    Error,
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenKind::Deps => write!(f, "deps"),
            TokenKind::Rdeps => write!(f, "rdeps"),
            TokenKind::Somepath => write!(f, "somepath"),
            TokenKind::Allpaths => write!(f, "allpaths"),
            TokenKind::Kind => write!(f, "kind"),
            TokenKind::Filter => write!(f, "filter"),
            TokenKind::Attr => write!(f, "attr"),
            TokenKind::Script => write!(f, "script"),
            TokenKind::Inputs => write!(f, "inputs"),
            TokenKind::Outputs => write!(f, "outputs"),
            TokenKind::Env => write!(f, "env"),
            TokenKind::CriticalPath => write!(f, "critical-path"),
            TokenKind::Intersect => write!(f, "intersect"),
            TokenKind::Union => write!(f, "union"),
            TokenKind::Except => write!(f, "except"),
            TokenKind::AllTargets => write!(f, "//..."),
            TokenKind::Ellipsis => write!(f, "..."),
            TokenKind::LParen => write!(f, "("),
            TokenKind::RParen => write!(f, ")"),
            TokenKind::Comma => write!(f, ","),
            TokenKind::Colon => write!(f, ":"),
            TokenKind::Star => write!(f, "*"),
            TokenKind::DoubleQuotedString => write!(f, "string"),
            TokenKind::SingleQuotedString => write!(f, "string"),
            TokenKind::Number => write!(f, "number"),
            TokenKind::Ident => write!(f, "identifier"),
            TokenKind::Error => write!(f, "error"),
        }
    }
}

/// A token with its text and source location
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,
    pub span: Range<usize>,
}

impl Token {
    /// Check if this token is a function keyword
    pub fn is_function(&self) -> bool {
        matches!(
            self.kind,
            TokenKind::Deps
                | TokenKind::Rdeps
                | TokenKind::Somepath
                | TokenKind::Allpaths
                | TokenKind::Kind
                | TokenKind::Filter
                | TokenKind::Attr
                | TokenKind::Script
                | TokenKind::Inputs
                | TokenKind::Outputs
                | TokenKind::Env
                | TokenKind::CriticalPath
        )
    }

    /// Check if this is a set operation
    pub fn is_set_op(&self) -> bool {
        matches!(
            self.kind,
            TokenKind::Intersect | TokenKind::Union | TokenKind::Except
        )
    }

    /// Get string content (strips quotes)
    pub fn string_value(&self) -> String {
        match self.kind {
            TokenKind::DoubleQuotedString | TokenKind::SingleQuotedString => {
                // Remove surrounding quotes
                self.text[1..self.text.len() - 1].to_string()
            }
            _ => self.text.clone(),
        }
    }

    /// Get numeric value
    pub fn number_value(&self) -> Option<usize> {
        if self.kind == TokenKind::Number {
            self.text.parse().ok()
        } else {
            None
        }
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.text)
    }
}

/// Query lexer
pub struct QueryLexer;

impl QueryLexer {
    /// Tokenize a query string
    pub fn tokenize(input: &str) -> Vec<Token> {
        let mut lexer = TokenKind::lexer(input);
        let mut tokens = Vec::new();

        while let Some(result) = lexer.next() {
            let kind = result.unwrap_or(TokenKind::Error);
            let span = lexer.span();
            let text = lexer.slice().to_string();
            tokens.push(Token { kind, text, span });
        }

        tokens
    }
}

/// Parse error with location information
#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: Range<usize>,
    pub source: String,
}

impl ParseError {
    pub fn new(message: impl Into<String>, span: Range<usize>, source: &str) -> Self {
        Self {
            message: message.into(),
            span,
            source: source.to_string(),
        }
    }

    /// Format error with source context
    pub fn format(&self) -> String {
        let line_start = self.source[..self.span.start]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let line_end = self.source[self.span.end..]
            .find('\n')
            .map(|i| self.span.end + i)
            .unwrap_or(self.source.len());

        let line = &self.source[line_start..line_end];
        let col = self.span.start - line_start;

        let pointer = format!("{}^{}",
            " ".repeat(col),
            "~".repeat(self.span.len().saturating_sub(1))
        );

        format!(
            "error: {}\n  |\n  | {}\n  | {}\n",
            self.message, line, pointer
        )
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format())
    }
}

impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_simple() {
        let tokens = QueryLexer::tokenize("deps(busybox, 2)");

        assert_eq!(tokens.len(), 6);
        assert_eq!(tokens[0].kind, TokenKind::Deps);
        assert_eq!(tokens[1].kind, TokenKind::LParen);
        assert_eq!(tokens[2].kind, TokenKind::Ident);
        assert_eq!(tokens[2].text, "busybox");
        assert_eq!(tokens[3].kind, TokenKind::Comma);
        assert_eq!(tokens[4].kind, TokenKind::Number);
        assert_eq!(tokens[4].text, "2");
        assert_eq!(tokens[5].kind, TokenKind::RParen);
    }

    #[test]
    fn test_tokenize_target_pattern() {
        let tokens = QueryLexer::tokenize("meta-core:busybox:do_compile");

        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens[0].kind, TokenKind::Ident);
        assert_eq!(tokens[0].text, "meta-core");
        assert_eq!(tokens[1].kind, TokenKind::Colon);
        assert_eq!(tokens[2].kind, TokenKind::Ident);
        assert_eq!(tokens[2].text, "busybox");
        assert_eq!(tokens[3].kind, TokenKind::Colon);
        assert_eq!(tokens[4].kind, TokenKind::Ident);
        assert_eq!(tokens[4].text, "do_compile");
    }

    #[test]
    fn test_tokenize_all_targets() {
        let tokens = QueryLexer::tokenize("//...");

        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::AllTargets);
    }

    #[test]
    fn test_tokenize_wildcard() {
        let tokens = QueryLexer::tokenize("*:busybox");

        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].kind, TokenKind::Star);
        assert_eq!(tokens[1].kind, TokenKind::Colon);
        assert_eq!(tokens[2].kind, TokenKind::Ident);
    }

    #[test]
    fn test_tokenize_string() {
        let tokens = QueryLexer::tokenize(r#"kind("go_binary", //...)"#);

        assert_eq!(tokens[0].kind, TokenKind::Kind);
        assert_eq!(tokens[2].kind, TokenKind::DoubleQuotedString);
        assert_eq!(tokens[2].string_value(), "go_binary");
    }

    #[test]
    fn test_tokenize_set_ops() {
        let tokens = QueryLexer::tokenize("deps(a) intersect deps(b)");

        assert!(tokens.iter().any(|t| t.kind == TokenKind::Intersect));
    }

    #[test]
    fn test_error_format() {
        let err = ParseError::new(
            "unexpected token",
            5..8,
            "deps(foo bar)",
        );

        let formatted = err.format();
        assert!(formatted.contains("unexpected token"));
        assert!(formatted.contains("deps(foo bar)"));
    }
}
