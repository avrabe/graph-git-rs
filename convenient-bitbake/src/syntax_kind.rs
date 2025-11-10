// Syntax kinds for BitBake CST
// Based on Rowan architecture (rust-analyzer approach)

use logos::Logos;

#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum SyntaxKind {
    // === Tokens (from lexer) ===

    // Assignment operators
    #[token("=")]
    EQ = 0,
    #[token(":=")]
    COLON_EQ,
    #[token("+=")]
    PLUS_EQ,
    #[token("=+")]
    EQ_PLUS,
    #[token(".=")]
    DOT_EQ,
    #[token("=.")]
    EQ_DOT,
    #[token("?=")]
    QUESTION_EQ,
    #[token("??=")]
    QUESTION_QUESTION_EQ,

    // Override syntax
    #[token(":append")]
    COLON_APPEND,
    #[token(":prepend")]
    COLON_PREPEND,
    #[token(":remove")]
    COLON_REMOVE,

    // Keywords
    #[token("inherit")]
    INHERIT_KW,
    #[token("include")]
    INCLUDE_KW,
    #[token("require")]
    REQUIRE_KW,
    #[token("export")]
    EXPORT_KW,
    #[token("def")]
    DEF_KW,
    #[token("python")]
    PYTHON_KW,

    // Identifiers and literals
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_\-]*", priority = 2)]
    IDENT,

    // Strings (double and single quoted)
    #[regex(r#""([^"\\]|\\.)*""#)]
    #[regex(r#"'([^'\\]|\\.)*'"#)]
    STRING,

    // Variable expansion
    #[regex(r"\$\{[^}]+\}")]
    VAR_EXPANSION,

    // Comments
    #[regex(r"#[^\n]*")]
    COMMENT,

    // Delimiters
    #[token("(")]
    L_PAREN,
    #[token(")")]
    R_PAREN,
    #[token("{")]
    L_BRACE,
    #[token("}")]
    R_BRACE,
    #[token("[")]
    L_BRACKET,
    #[token("]")]
    R_BRACKET,

    // Special tokens
    #[token(";")]
    SEMICOLON,
    #[token(":")]
    COLON,
    #[token("\\")]
    BACKSLASH,
    #[token("\n")]
    NEWLINE,

    // Whitespace (not newline)
    #[regex(r"[ \t]+")]
    WHITESPACE,

    // Error token (logos 0.14+ doesn't use #[error])
    ERROR_TOKEN,

    // === Composite nodes (not from lexer) ===

    // File structure
    ROOT = 100,

    // Variable assignments
    VARIABLE_ASSIGNMENT,
    VARIABLE_NAME,
    VARIABLE_VALUE,
    OVERRIDE_LIST,

    // Statements
    INHERIT_STMT,
    INCLUDE_STMT,
    REQUIRE_STMT,
    EXPORT_STMT,

    // Functions
    SHELL_FUNCTION,
    PYTHON_FUNCTION,
    ANONYMOUS_PYTHON,
    FUNCTION_BODY,

    // Expressions
    STRING_EXPR,
    VAR_REF,
    CONCAT_EXPR,

    // Error recovery
    ERROR,

    // End marker
    EOF,

    // Trivia
    TRIVIA,
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BitbakeLang {}

impl rowan::Language for BitbakeLang {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        assert!(raw.0 <= SyntaxKind::TRIVIA as u16);
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub type SyntaxNode = rowan::SyntaxNode<BitbakeLang>;
pub type SyntaxToken = rowan::SyntaxToken<BitbakeLang>;
pub type SyntaxElement = rowan::SyntaxElement<BitbakeLang>;

impl SyntaxKind {
    pub fn is_trivia(self) -> bool {
        matches!(self, Self::WHITESPACE | Self::COMMENT | Self::NEWLINE)
    }

    pub fn is_assignment_op(self) -> bool {
        matches!(
            self,
            Self::EQ
                | Self::COLON_EQ
                | Self::PLUS_EQ
                | Self::EQ_PLUS
                | Self::DOT_EQ
                | Self::EQ_DOT
                | Self::QUESTION_EQ
                | Self::QUESTION_QUESTION_EQ
        )
    }

    pub fn is_keyword(self) -> bool {
        matches!(
            self,
            Self::INHERIT_KW
                | Self::INCLUDE_KW
                | Self::REQUIRE_KW
                | Self::EXPORT_KW
                | Self::DEF_KW
                | Self::PYTHON_KW
        )
    }
}
