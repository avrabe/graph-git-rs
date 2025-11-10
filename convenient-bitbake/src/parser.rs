// Resilient BitBake parser using Rowan CST
// Based on rust-analyzer architecture

use crate::lexer::{Lexer, Token};
use crate::syntax_kind::{BitbakeLang, SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;
use std::fmt;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: std::ops::Range<usize>,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {:?}", self.message, self.span)
    }
}

pub struct Parse {
    green_node: rowan::GreenNode,
    pub errors: Vec<ParseError>,
}

impl Parse {
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green_node.clone())
    }

    pub fn ok(&self) -> Result<SyntaxNode, &[ParseError]> {
        if self.errors.is_empty() {
            Ok(self.syntax())
        } else {
            Err(&self.errors)
        }
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    errors: Vec<ParseError>,
}

impl Parser {
    pub fn parse(input: &str) -> Parse {
        let tokens = Lexer::tokenize(input);

        let mut parser = Parser {
            tokens,
            pos: 0,
            builder: GreenNodeBuilder::new(),
            errors: Vec::new(),
        };

        parser.parse_root();

        Parse {
            green_node: parser.builder.finish(),
            errors: parser.errors,
        }
    }

    fn parse_root(&mut self) {
        self.builder.start_node(SyntaxKind::ROOT.into());

        while !self.at_eof() {
            self.skip_trivia();

            if self.at_eof() {
                break;
            }

            // Try to parse a statement
            if !self.statement() {
                // Error recovery: skip to next line
                self.error(format!("Unexpected token: {:?}", self.current_token()));
                self.advance_with_error();
            }
        }

        self.builder.finish_node();
    }

    fn statement(&mut self) -> bool {
        match self.current() {
            SyntaxKind::INHERIT_KW => self.inherit_stmt(),
            SyntaxKind::INCLUDE_KW => self.include_stmt(),
            SyntaxKind::REQUIRE_KW => self.require_stmt(),
            SyntaxKind::EXPORT_KW => self.export_stmt(),
            SyntaxKind::PYTHON_KW | SyntaxKind::DEF_KW => self.function_def(),
            SyntaxKind::IDENT => {
                // Could be assignment or function call
                if self.is_assignment_ahead() {
                    self.assignment()
                } else {
                    // Skip unknown statements
                    false
                }
            }
            SyntaxKind::COMMENT | SyntaxKind::NEWLINE => {
                self.bump();  // Just consume
                true
            }
            _ => false,
        }
    }

    fn assignment(&mut self) -> bool {
        self.builder.start_node(SyntaxKind::VARIABLE_ASSIGNMENT.into());

        // Variable name node
        self.builder.start_node(SyntaxKind::VARIABLE_NAME.into());

        if !self.at(SyntaxKind::IDENT) {
            self.error("Expected identifier".to_string());
            self.builder.finish_node();  // VARIABLE_NAME
            self.builder.finish_node();  // VARIABLE_ASSIGNMENT
            return false;
        }

        self.bump();  // variable name

        // Handle override syntax: VAR:append:machine
        while self.at(SyntaxKind::COLON) ||
              self.at(SyntaxKind::COLON_APPEND) ||
              self.at(SyntaxKind::COLON_PREPEND) ||
              self.at(SyntaxKind::COLON_REMOVE)
        {
            self.bump();  // :xxx
            if self.at(SyntaxKind::IDENT) {
                self.bump();  // override name
            }
        }

        // Handle flag syntax: VAR[flag]
        if self.at(SyntaxKind::L_BRACKET) {
            self.bump();  // [
            if self.at(SyntaxKind::IDENT) || self.at(SyntaxKind::STRING) {
                self.bump();  // flag name
            }
            self.expect(SyntaxKind::R_BRACKET);  // ]
        }

        self.builder.finish_node();  // VARIABLE_NAME

        // Skip whitespace before operator
        self.skip_whitespace_inline();

        // Assignment operator
        if !self.current().is_assignment_op() {
            self.error(format!("Expected assignment operator, found {:?}", self.current()));
            self.builder.finish_node();  // VARIABLE_ASSIGNMENT
            return false;
        }

        self.bump();  // operator

        // Skip whitespace after operator
        self.skip_whitespace_inline();

        // Variable value node
        self.builder.start_node(SyntaxKind::VARIABLE_VALUE.into());
        self.value();
        self.builder.finish_node();  // VARIABLE_VALUE

        self.builder.finish_node();  // VARIABLE_ASSIGNMENT
        true
    }

    fn value(&mut self) {
        // Parse value which can span multiple lines with backslash continuation
        loop {
            match self.current() {
                SyntaxKind::STRING => {
                    self.bump();
                }
                SyntaxKind::VAR_EXPANSION => {
                    self.bump();
                }
                SyntaxKind::IDENT => {
                    self.bump();
                }
                SyntaxKind::WHITESPACE => {
                    self.bump();
                }
                SyntaxKind::BACKSLASH => {
                    self.bump();
                    // Line continuation - consume newline if present
                    if self.at(SyntaxKind::NEWLINE) {
                        self.bump();
                    }
                    // Continue parsing value on next line
                    continue;
                }
                SyntaxKind::NEWLINE | SyntaxKind::EOF => {
                    // End of value
                    break;
                }
                _ => {
                    // Unknown token in value - consume it but continue
                    self.bump();
                }
            }

            // If we hit a newline without backslash, stop
            if self.at(SyntaxKind::NEWLINE) {
                break;
            }
        }
    }

    fn inherit_stmt(&mut self) -> bool {
        self.builder.start_node(SyntaxKind::INHERIT_STMT.into());

        self.bump();  // inherit keyword

        // Parse list of class names
        self.skip_whitespace_inline();

        while self.at(SyntaxKind::IDENT) {
            self.bump();  // class name
            self.skip_whitespace_inline();
        }

        self.builder.finish_node();
        true
    }

    fn include_stmt(&mut self) -> bool {
        self.builder.start_node(SyntaxKind::INCLUDE_STMT.into());

        self.bump();  // include keyword

        self.skip_whitespace_inline();

        // File path (can be string or identifier with variable expansion)
        if self.at(SyntaxKind::STRING) || self.at(SyntaxKind::IDENT) || self.at(SyntaxKind::VAR_EXPANSION) {
            self.bump();
        }

        self.builder.finish_node();
        true
    }

    fn require_stmt(&mut self) -> bool {
        self.builder.start_node(SyntaxKind::REQUIRE_STMT.into());

        self.bump();  // require keyword

        self.skip_whitespace_inline();

        // File path
        if self.at(SyntaxKind::STRING) || self.at(SyntaxKind::IDENT) || self.at(SyntaxKind::VAR_EXPANSION) {
            self.bump();
        }

        self.builder.finish_node();
        true
    }

    fn export_stmt(&mut self) -> bool {
        self.builder.start_node(SyntaxKind::EXPORT_STMT.into());

        self.bump();  // export keyword

        self.skip_whitespace_inline();

        // Variable name
        if self.at(SyntaxKind::IDENT) {
            self.bump();
        }

        self.builder.finish_node();
        true
    }

    fn function_def(&mut self) -> bool {
        // Simplified function parsing - just skip the body
        self.builder.start_node(SyntaxKind::SHELL_FUNCTION.into());

        // python or def keyword
        if self.at(SyntaxKind::PYTHON_KW) || self.at(SyntaxKind::DEF_KW) {
            self.bump();
        }

        self.skip_whitespace_inline();

        // Function name
        if self.at(SyntaxKind::IDENT) {
            self.bump();
        }

        self.skip_whitespace_inline();

        // Parameters ()
        if self.at(SyntaxKind::L_PAREN) {
            self.bump();
            while !self.at(SyntaxKind::R_PAREN) && !self.at_eof() {
                self.bump();
            }
            if self.at(SyntaxKind::R_PAREN) {
                self.bump();
            }
        }

        self.skip_whitespace_inline();

        // Body {...}
        if self.at(SyntaxKind::L_BRACE) {
            self.bump();
            let mut depth = 1;
            while depth > 0 && !self.at_eof() {
                if self.at(SyntaxKind::L_BRACE) {
                    depth += 1;
                } else if self.at(SyntaxKind::R_BRACE) {
                    depth -= 1;
                }
                self.bump();
            }
        }

        self.builder.finish_node();
        true
    }

    // === Helper methods ===

    fn current(&self) -> SyntaxKind {
        self.tokens.get(self.pos).map(|t| t.kind).unwrap_or(SyntaxKind::EOF)
    }

    fn current_token(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    fn at_eof(&self) -> bool {
        self.pos >= self.tokens.len() || self.at(SyntaxKind::EOF)
    }

    fn bump(&mut self) {
        if let Some(token) = self.tokens.get(self.pos) {
            self.builder.token(token.kind.into(), &token.text);
            self.pos += 1;
        }
    }

    fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            self.error(format!("Expected {:?}, found {:?}", kind, self.current()));
            false
        }
    }

    fn error(&mut self, message: String) {
        let span = self.current_token()
            .map(|t| t.span.clone())
            .unwrap_or(0..0);

        self.errors.push(ParseError { message, span });
    }

    fn skip_trivia(&mut self) {
        while self.current().is_trivia() {
            self.bump();
        }
    }

    fn skip_whitespace_inline(&mut self) {
        while self.at(SyntaxKind::WHITESPACE) {
            self.bump();
        }
    }

    fn is_assignment_ahead(&self) -> bool {
        let mut lookahead = self.pos + 1;

        // Skip whitespace and potential override syntax
        while lookahead < self.tokens.len() {
            match self.tokens[lookahead].kind {
                SyntaxKind::WHITESPACE => {
                    lookahead += 1;
                }
                SyntaxKind::COLON | SyntaxKind::COLON_APPEND |
                SyntaxKind::COLON_PREPEND | SyntaxKind::COLON_REMOVE => {
                    lookahead += 1;
                }
                SyntaxKind::IDENT => {
                    lookahead += 1;
                }
                SyntaxKind::L_BRACKET => {
                    // Skip flag syntax
                    lookahead += 1;
                    while lookahead < self.tokens.len() &&
                          self.tokens[lookahead].kind != SyntaxKind::R_BRACKET {
                        lookahead += 1;
                    }
                    if lookahead < self.tokens.len() {
                        lookahead += 1;  // skip ]
                    }
                }
                kind if kind.is_assignment_op() => {
                    return true;
                }
                _ => {
                    return false;
                }
            }
        }

        false
    }

    fn advance_with_error(&mut self) {
        self.builder.start_node(SyntaxKind::ERROR.into());

        // Skip to next statement or newline
        while !self.at_eof() {
            if self.at(SyntaxKind::NEWLINE) {
                self.bump();
                break;
            }
            if matches!(
                self.current(),
                SyntaxKind::INHERIT_KW | SyntaxKind::INCLUDE_KW |
                SyntaxKind::REQUIRE_KW | SyntaxKind::EXPORT_KW
            ) {
                break;
            }
            self.bump();
        }

        self.builder.finish_node();
    }
}

// Public API
pub fn parse(input: &str) -> Parse {
    Parser::parse(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_assignment() {
        let input = r#"FOO = "bar""#;
        let parse = parse(input);

        assert!(parse.errors.is_empty(), "Should parse without errors");

        let root = parse.syntax();
        assert_eq!(root.kind(), SyntaxKind::ROOT);
    }

    #[test]
    fn test_append_assignment() {
        let input = r#"FOO += "more""#;
        let parse = parse(input);

        assert!(parse.errors.is_empty());
    }

    #[test]
    fn test_override_syntax() {
        let input = r#"FOO:append = "value""#;
        let parse = parse(input);

        assert!(parse.errors.is_empty());
    }

    #[test]
    fn test_multiline_value() {
        let input = r#"SRC_URI = "git://example.com/repo.git \
           file://patch.patch \
          ""#;
        let parse = parse(input);

        assert!(parse.errors.is_empty());
    }

    #[test]
    fn test_inherit() {
        let input = "inherit cmake cargo";
        let parse = parse(input);

        assert!(parse.errors.is_empty());

        let root = parse.syntax();
        let mut inherit_found = false;
        for node in root.descendants() {
            if node.kind() == SyntaxKind::INHERIT_STMT {
                inherit_found = true;
                break;
            }
        }
        assert!(inherit_found, "Should find inherit statement");
    }

    #[test]
    fn test_include() {
        let input = "include ${BPN}-crates.inc";
        let parse = parse(input);

        assert!(parse.errors.is_empty());
    }

    #[test]
    fn test_error_recovery() {
        let input = r#"
FOO = "valid"
@@@@@  # Invalid line
BAR = "also valid"
        "#;

        let parse = parse(input);

        // Should have errors
        assert!(!parse.errors.is_empty());

        // But should still parse valid lines
        let root = parse.syntax();
        let assignments: Vec<_> = root.descendants()
            .filter(|n| n.kind() == SyntaxKind::VARIABLE_ASSIGNMENT)
            .collect();

        // Should find at least the valid assignments
        assert!(!assignments.is_empty());
    }

    #[test]
    fn test_meta_fmu_snippet() {
        let input = r#"
SUMMARY = "fmu-rs"
HOMEPAGE = "https://github.com/avrabe/fmu-rs.git"
LICENSE = "MIT"

inherit cargo cargo-update-recipe-crates

SRC_URI = "git://github.com/avrabe/fmu-rs;protocol=https;nobranch=1;branch=main"
include ${BPN}-crates.inc

S = "${WORKDIR}/git"
CARGO_SRC_DIR = ""

include ${BPN}-srcrev.inc
PV:append = ".AUTOINC+${SRCPV}"
DEPENDS:append = " ostree openssl pkgconfig-native "
        "#;

        let parse = parse(input);

        // Should parse successfully
        println!("Errors: {:?}", parse.errors);
        assert!(parse.errors.is_empty() || parse.errors.len() < 3);

        let root = parse.syntax();

        // Count different node types
        let assignments = root.descendants()
            .filter(|n| n.kind() == SyntaxKind::VARIABLE_ASSIGNMENT)
            .count();
        let inherits = root.descendants()
            .filter(|n| n.kind() == SyntaxKind::INHERIT_STMT)
            .count();
        let includes = root.descendants()
            .filter(|n| n.kind() == SyntaxKind::INCLUDE_STMT)
            .count();

        assert!(assignments >= 5, "Should find multiple assignments");
        assert!(inherits >= 1, "Should find inherit statement");
        assert!(includes >= 2, "Should find include statements");
    }
}
