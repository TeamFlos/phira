use logos::{Lexer, Logos};

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(error = String)]
#[logos(skip r"[ \t\n\f]+")]
pub enum Token {
    #[regex(r"\{([^}]|}})*}", text_block)]
    Text(String),
    #[regex(r#""[^"]*""#, quoted)]
    Quoted(String),

    #[token("global")]
    Global,

    #[token("#>if-no-v2")]
    IfNoV2,

    #[token("#>if")]
    If,

    #[token("#>else")]
    Else,

    #[token("#>elif")]
    ElseIf,

    #[token("#>fi")]
    EndIf,

    #[regex("(#>)?[@a-zA-Z$_][a-zA-Z0-9-$_]*", |lex| lex.slice().to_owned())]
    Ident(String),

    #[regex("-?[0-9]*[.]?[0-9]+(?:[eE][+-]?[0-9]+)?", |lex| lex.slice().parse().ok())]
    Number(f32),

    #[regex("true|false", |lex| lex.slice() == "true")]
    Bool(bool),

    #[regex("#([^>][^\n]*)?", |_| logos::Skip)]
    Comment,

    #[token("(")]
    LBrace,
    #[token(")")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token("=")]
    Assign,
    #[token(".")]
    Period,
    #[token("+")]
    Add,
    #[token("-")]
    Sub,
    #[token("*")]
    Mul,
    #[token("/")]
    Div,
    #[token("<=")]
    Le,
    #[token(">=")]
    Ge,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("==")]
    Eq,
    #[token("!=")]
    Neq,
}

fn text_block(lexer: &Lexer<Token>) -> String {
    let s = lexer.slice().replace("}}", "}");
    let s = s[1..(s.len() - 1)].trim_start_matches(' ').trim_end();
    if s.starts_with('\n') {
        let ident: String = s.chars().take_while(|&it| it == '\n' || it == ' ' || it == '\t').collect();
        s[ident.len()..].replace(&ident, "\n")
    } else {
        s.to_owned()
    }
}

fn quoted(lexer: &Lexer<Token>) -> String {
    let s = lexer.slice();
    s[1..(s.len() - 1)].to_owned()
}
