/// JS lexer — tokenizes source into tokens.

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Number(f64),
    Str(String),
    Ident(String),
    Bool(bool),
    Null,
    Undefined,

    // Keywords
    Var,
    Let,
    Const,
    Function,
    Return,
    If,
    Else,
    While,
    For,
    Break,
    Continue,
    New,
    This,
    Typeof,
    Void,
    Delete,
    In,
    Of,
    Throw,
    Try,
    Catch,
    Finally,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    StarStar,       // **
    Eq,             // =
    EqEq,           // ==
    EqEqEq,         // ===
    BangEq,         // !=
    BangEqEq,       // !==
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,            // &&
    Or,             // ||
    Bang,           // !
    BitAnd,         // &
    BitOr,          // |
    BitXor,         // ^
    BitNot,         // ~
    Shl,            // <<
    Shr,            // >>
    UShr,           // >>>
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PlusPlus,
    MinusMinus,
    Question,       // ?
    QuestionDot,    // ?.
    NullCoalesce,   // ??
    Arrow,          // =>
    Spread,         // ...

    // Delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Dot,
    Semi,
    Colon,

    // Special
    Eof,
}

pub struct Lexer {
    src: Vec<char>,
    pos: usize,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Self {
            src: source.chars().collect(),
            pos: 0,
        }
    }

    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token();
            if tok == Token::Eof {
                tokens.push(tok);
                break;
            }
            tokens.push(tok);
        }
        tokens
    }

    fn peek(&self) -> char {
        if self.pos < self.src.len() { self.src[self.pos] } else { '\0' }
    }

    fn peek_at(&self, offset: usize) -> char {
        let i = self.pos + offset;
        if i < self.src.len() { self.src[i] } else { '\0' }
    }

    fn advance(&mut self) -> char {
        let c = self.peek();
        self.pos += 1;
        c
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.src.len() {
            let c = self.peek();
            if c.is_whitespace() {
                self.advance();
            } else if c == '/' && self.peek_at(1) == '/' {
                // Line comment
                while self.pos < self.src.len() && self.peek() != '\n' {
                    self.advance();
                }
            } else if c == '/' && self.peek_at(1) == '*' {
                // Block comment
                self.advance();
                self.advance();
                while self.pos < self.src.len() {
                    if self.peek() == '*' && self.peek_at(1) == '/' {
                        self.advance();
                        self.advance();
                        break;
                    }
                    self.advance();
                }
            } else {
                break;
            }
        }
    }

    fn read_string(&mut self, quote: char) -> Token {
        self.advance(); // skip opening quote
        let mut s = String::new();
        while self.pos < self.src.len() && self.peek() != quote {
            let c = self.advance();
            if c == '\\' {
                let esc = self.advance();
                match esc {
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    'r' => s.push('\r'),
                    '\\' => s.push('\\'),
                    '\'' => s.push('\''),
                    '"' => s.push('"'),
                    '`' => s.push('`'),
                    '0' => s.push('\0'),
                    _ => { s.push('\\'); s.push(esc); }
                }
            } else {
                s.push(c);
            }
        }
        self.advance(); // skip closing quote
        Token::Str(s)
    }

    fn read_template(&mut self) -> Token {
        self.advance(); // skip `
        let mut s = String::new();
        while self.pos < self.src.len() && self.peek() != '`' {
            let c = self.advance();
            if c == '\\' {
                let esc = self.advance();
                match esc {
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    _ => { s.push(esc); }
                }
            } else {
                s.push(c);
            }
        }
        self.advance(); // skip `
        Token::Str(s)
    }

    fn read_number(&mut self) -> Token {
        let start = self.pos;
        // Check for hex/octal/binary
        if self.peek() == '0' {
            let next = self.peek_at(1);
            if next == 'x' || next == 'X' {
                self.advance(); self.advance();
                while self.pos < self.src.len() && self.peek().is_ascii_hexdigit() {
                    self.advance();
                }
                let s: String = self.src[start..self.pos].iter().collect();
                let n = u64::from_str_radix(&s[2..], 16).unwrap_or(0) as f64;
                return Token::Number(n);
            }
        }

        while self.pos < self.src.len() && (self.peek().is_ascii_digit() || self.peek() == '.') {
            self.advance();
        }
        // Exponent
        if self.pos < self.src.len() && (self.peek() == 'e' || self.peek() == 'E') {
            self.advance();
            if self.peek() == '+' || self.peek() == '-' { self.advance(); }
            while self.pos < self.src.len() && self.peek().is_ascii_digit() {
                self.advance();
            }
        }
        let s: String = self.src[start..self.pos].iter().collect();
        Token::Number(s.parse::<f64>().unwrap_or(f64::NAN))
    }

    fn read_ident(&mut self) -> Token {
        let start = self.pos;
        while self.pos < self.src.len() && (self.peek().is_alphanumeric() || self.peek() == '_' || self.peek() == '$') {
            self.advance();
        }
        let s: String = self.src[start..self.pos].iter().collect();
        match s.as_str() {
            "var" => Token::Var,
            "let" => Token::Let,
            "const" => Token::Const,
            "function" => Token::Function,
            "return" => Token::Return,
            "if" => Token::If,
            "else" => Token::Else,
            "while" => Token::While,
            "for" => Token::For,
            "break" => Token::Break,
            "continue" => Token::Continue,
            "new" => Token::New,
            "this" => Token::This,
            "typeof" => Token::Typeof,
            "void" => Token::Void,
            "delete" => Token::Delete,
            "in" => Token::In,
            "of" => Token::Of,
            "true" => Token::Bool(true),
            "false" => Token::Bool(false),
            "null" => Token::Null,
            "undefined" => Token::Undefined,
            "throw" => Token::Throw,
            "try" => Token::Try,
            "catch" => Token::Catch,
            "finally" => Token::Finally,
            _ => Token::Ident(s),
        }
    }

    fn next_token(&mut self) -> Token {
        self.skip_whitespace();
        if self.pos >= self.src.len() {
            return Token::Eof;
        }

        let c = self.peek();

        // Strings
        if c == '\'' || c == '"' {
            return self.read_string(c);
        }
        if c == '`' {
            return self.read_template();
        }

        // Numbers
        if c.is_ascii_digit() || (c == '.' && self.peek_at(1).is_ascii_digit()) {
            return self.read_number();
        }

        // Identifiers and keywords
        if c.is_alphabetic() || c == '_' || c == '$' {
            return self.read_ident();
        }

        // Operators and punctuation
        self.advance();
        match c {
            '+' => {
                if self.peek() == '+' { self.advance(); Token::PlusPlus }
                else if self.peek() == '=' { self.advance(); Token::PlusEq }
                else { Token::Plus }
            }
            '-' => {
                if self.peek() == '-' { self.advance(); Token::MinusMinus }
                else if self.peek() == '=' { self.advance(); Token::MinusEq }
                else { Token::Minus }
            }
            '*' => {
                if self.peek() == '*' { self.advance(); Token::StarStar }
                else if self.peek() == '=' { self.advance(); Token::StarEq }
                else { Token::Star }
            }
            '/' => {
                if self.peek() == '=' { self.advance(); Token::SlashEq }
                else { Token::Slash }
            }
            '%' => Token::Percent,
            '=' => {
                if self.peek() == '=' {
                    self.advance();
                    if self.peek() == '=' { self.advance(); Token::EqEqEq }
                    else { Token::EqEq }
                } else if self.peek() == '>' {
                    self.advance(); Token::Arrow
                } else {
                    Token::Eq
                }
            }
            '!' => {
                if self.peek() == '=' {
                    self.advance();
                    if self.peek() == '=' { self.advance(); Token::BangEqEq }
                    else { Token::BangEq }
                } else {
                    Token::Bang
                }
            }
            '<' => {
                if self.peek() == '=' { self.advance(); Token::LtEq }
                else if self.peek() == '<' { self.advance(); Token::Shl }
                else { Token::Lt }
            }
            '>' => {
                if self.peek() == '=' { self.advance(); Token::GtEq }
                else if self.peek() == '>' {
                    self.advance();
                    if self.peek() == '>' { self.advance(); Token::UShr }
                    else { Token::Shr }
                }
                else { Token::Gt }
            }
            '&' => {
                if self.peek() == '&' { self.advance(); Token::And }
                else { Token::BitAnd }
            }
            '|' => {
                if self.peek() == '|' { self.advance(); Token::Or }
                else { Token::BitOr }
            }
            '^' => Token::BitXor,
            '~' => Token::BitNot,
            '?' => {
                if self.peek() == '.' { self.advance(); Token::QuestionDot }
                else if self.peek() == '?' { self.advance(); Token::NullCoalesce }
                else { Token::Question }
            }
            '.' => {
                if self.peek() == '.' && self.peek_at(1) == '.' {
                    self.advance(); self.advance(); Token::Spread
                } else {
                    Token::Dot
                }
            }
            '(' => Token::LParen,
            ')' => Token::RParen,
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            '[' => Token::LBracket,
            ']' => Token::RBracket,
            ',' => Token::Comma,
            ';' => Token::Semi,
            ':' => Token::Colon,
            _ => Token::Eof, // unknown char, skip
        }
    }
}
