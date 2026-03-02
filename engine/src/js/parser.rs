use super::lexer::Token;

#[derive(Debug, Clone)]
pub enum Expr {
    Number(f64),
    Str(String),
    Bool(bool),
    Null,
    Undefined,
    Ident(String),
    This,

    // Binary ops
    BinOp(Box<Expr>, BinOp, Box<Expr>),
    // Unary
    UnaryOp(UnaryOp, Box<Expr>),
    // Assignment: target = value
    Assign(Box<Expr>, Box<Expr>),
    // Compound assignment: target += value
    CompoundAssign(Box<Expr>, BinOp, Box<Expr>),
    // Member access: obj.prop
    Member(Box<Expr>, String),
    // Computed access: obj[expr]
    Index(Box<Expr>, Box<Expr>),
    // Function call: callee(args...)
    Call(Box<Expr>, Vec<Expr>),
    // new Constructor(args...)
    New(Box<Expr>, Vec<Expr>),
    // Array literal
    Array(Vec<Expr>),
    // Object literal
    Object(Vec<(String, Expr)>),
    // Function expression
    FunctionExpr {
        name: Option<String>,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    // Arrow function
    Arrow {
        params: Vec<String>,
        body: Box<ArrowBody>,
    },
    // Ternary
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    // typeof x
    Typeof(Box<Expr>),
    // void x
    Void(Box<Expr>),
    // Prefix ++/--
    PreIncDec(Box<Expr>, bool), // true = ++
    // Postfix ++/--
    PostIncDec(Box<Expr>, bool),
}

#[derive(Debug, Clone)]
pub enum ArrowBody {
    Expr(Expr),
    Block(Vec<Stmt>),
}

#[derive(Debug, Clone)]
pub enum BinOp {
    Add, Sub, Mul, Div, Mod, Pow,
    Eq, Neq, StrictEq, StrictNeq,
    Lt, Gt, Lte, Gte,
    And, Or, NullCoalesce,
    BitAnd, BitOr, BitXor, Shl, Shr, UShr,
    In,
}

#[derive(Debug, Clone)]
pub enum UnaryOp {
    Neg, Not, BitNot, Typeof, Void, Delete,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr),
    VarDecl(String, Option<Expr>),
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        update: Option<Expr>,
        body: Box<Stmt>,
    },
    Function {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    Return(Option<Expr>),
    Break,
    Continue,
    Throw(Expr),
    Try {
        body: Vec<Stmt>,
        catch_param: Option<String>,
        catch_body: Option<Vec<Stmt>>,
        finally_body: Option<Vec<Stmt>>,
    },
    Empty,
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        while !self.at_end() {
            if self.peek() == &Token::Semi {
                self.advance();
                continue;
            }
            stmts.push(self.parse_stmt());
        }
        stmts
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn peek_at(&self, offset: usize) -> &Token {
        self.tokens.get(self.pos + offset).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) {
        let tok = self.advance();
        if &tok != expected {
            // Lenient: don't panic, just continue (ASI-like behavior)
        }
    }

    fn at_end(&self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    fn eat_semis(&mut self) {
        while self.peek() == &Token::Semi {
            self.advance();
        }
    }

    // ── Statements ──────────────────────────────────────────────────────

    fn parse_stmt(&mut self) -> Stmt {
        match self.peek().clone() {
            Token::Var | Token::Let | Token::Const => self.parse_var_decl(),
            Token::Function => {
                if matches!(self.peek_at(1), Token::Ident(_)) {
                    self.parse_function_decl()
                } else {
                    let expr = self.parse_expr();
                    self.eat_semis();
                    Stmt::Expr(expr)
                }
            }
            Token::LBrace => self.parse_block(),
            Token::If => self.parse_if(),
            Token::While => self.parse_while(),
            Token::For => self.parse_for(),
            Token::Return => self.parse_return(),
            Token::Break => { self.advance(); self.eat_semis(); Stmt::Break }
            Token::Continue => { self.advance(); self.eat_semis(); Stmt::Continue }
            Token::Throw => self.parse_throw(),
            Token::Try => self.parse_try(),
            _ => {
                let expr = self.parse_expr();
                self.eat_semis();
                Stmt::Expr(expr)
            }
        }
    }

    fn parse_var_decl(&mut self) -> Stmt {
        self.advance(); // var/let/const
        if let Token::Ident(name) = self.advance() {
            let init = if self.peek() == &Token::Eq {
                self.advance();
                Some(self.parse_expr())
            } else {
                None
            };
            self.eat_semis();
            Stmt::VarDecl(name, init)
        } else {
            Stmt::Empty
        }
    }

    fn parse_function_decl(&mut self) -> Stmt {
        self.advance(); // function
        let name = if let Token::Ident(n) = self.advance() { n } else { String::new() };
        self.expect(&Token::LParen);
        let params = self.parse_param_list();
        self.expect(&Token::RParen);
        let body = self.parse_block_stmts();
        Stmt::Function { name, params, body }
    }

    fn parse_param_list(&mut self) -> Vec<String> {
        let mut params = Vec::new();
        while self.peek() != &Token::RParen && !self.at_end() {
            if let Token::Ident(name) = self.advance() {
                params.push(name);
            }
            if self.peek() == &Token::Comma {
                self.advance();
            }
        }
        params
    }

    fn parse_block(&mut self) -> Stmt {
        Stmt::Block(self.parse_block_stmts())
    }

    fn parse_block_stmts(&mut self) -> Vec<Stmt> {
        self.expect(&Token::LBrace);
        let mut stmts = Vec::new();
        while self.peek() != &Token::RBrace && !self.at_end() {
            if self.peek() == &Token::Semi {
                self.advance();
                continue;
            }
            stmts.push(self.parse_stmt());
        }
        self.expect(&Token::RBrace);
        stmts
    }

    fn parse_if(&mut self) -> Stmt {
        self.advance(); // if
        self.expect(&Token::LParen);
        let cond = self.parse_expr();
        self.expect(&Token::RParen);
        let then = self.parse_stmt();
        let else_ = if self.peek() == &Token::Else {
            self.advance();
            Some(Box::new(self.parse_stmt()))
        } else {
            None
        };
        Stmt::If(cond, Box::new(then), else_)
    }

    fn parse_while(&mut self) -> Stmt {
        self.advance(); // while
        self.expect(&Token::LParen);
        let cond = self.parse_expr();
        self.expect(&Token::RParen);
        let body = self.parse_stmt();
        Stmt::While(cond, Box::new(body))
    }

    fn parse_for(&mut self) -> Stmt {
        self.advance(); // for
        self.expect(&Token::LParen);
        let init = if self.peek() == &Token::Semi {
            None
        } else {
            Some(Box::new(self.parse_stmt()))
        };
        // init already consumed its semicolon
        let cond = if self.peek() == &Token::Semi {
            None
        } else {
            Some(self.parse_expr())
        };
        self.eat_semis();
        let update = if self.peek() == &Token::RParen {
            None
        } else {
            Some(self.parse_expr())
        };
        self.expect(&Token::RParen);
        let body = self.parse_stmt();
        Stmt::For { init, cond, update, body: Box::new(body) }
    }

    fn parse_return(&mut self) -> Stmt {
        self.advance(); // return
        if self.peek() == &Token::Semi || self.peek() == &Token::RBrace || self.at_end() {
            self.eat_semis();
            Stmt::Return(None)
        } else {
            let expr = self.parse_expr();
            self.eat_semis();
            Stmt::Return(Some(expr))
        }
    }

    fn parse_throw(&mut self) -> Stmt {
        self.advance(); // throw
        let expr = self.parse_expr();
        self.eat_semis();
        Stmt::Throw(expr)
    }

    fn parse_try(&mut self) -> Stmt {
        self.advance(); // try
        let body = self.parse_block_stmts();
        let (catch_param, catch_body) = if self.peek() == &Token::Catch {
            self.advance();
            let param = if self.peek() == &Token::LParen {
                self.advance();
                let p = if let Token::Ident(n) = self.advance() { Some(n) } else { None };
                self.expect(&Token::RParen);
                p
            } else {
                None
            };
            let cb = self.parse_block_stmts();
            (param, Some(cb))
        } else {
            (None, None)
        };
        let finally_body = if self.peek() == &Token::Finally {
            self.advance();
            Some(self.parse_block_stmts())
        } else {
            None
        };
        Stmt::Try { body, catch_param, catch_body, finally_body }
    }

    // ── Expressions (precedence climbing) ───────────────────────────────

    fn parse_expr(&mut self) -> Expr {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Expr {
        let left = self.parse_ternary();
        match self.peek() {
            Token::Eq => {
                self.advance();
                let right = self.parse_assignment();
                Expr::Assign(Box::new(left), Box::new(right))
            }
            Token::PlusEq => {
                self.advance();
                let right = self.parse_assignment();
                Expr::CompoundAssign(Box::new(left), BinOp::Add, Box::new(right))
            }
            Token::MinusEq => {
                self.advance();
                let right = self.parse_assignment();
                Expr::CompoundAssign(Box::new(left), BinOp::Sub, Box::new(right))
            }
            Token::StarEq => {
                self.advance();
                let right = self.parse_assignment();
                Expr::CompoundAssign(Box::new(left), BinOp::Mul, Box::new(right))
            }
            Token::SlashEq => {
                self.advance();
                let right = self.parse_assignment();
                Expr::CompoundAssign(Box::new(left), BinOp::Div, Box::new(right))
            }
            _ => left,
        }
    }

    fn parse_ternary(&mut self) -> Expr {
        let cond = self.parse_nullish();
        if self.peek() == &Token::Question {
            self.advance();
            let then = self.parse_assignment();
            self.expect(&Token::Colon);
            let else_ = self.parse_assignment();
            Expr::Ternary(Box::new(cond), Box::new(then), Box::new(else_))
        } else {
            cond
        }
    }

    fn parse_nullish(&mut self) -> Expr {
        let mut left = self.parse_or();
        while self.peek() == &Token::NullCoalesce {
            self.advance();
            let right = self.parse_or();
            left = Expr::BinOp(Box::new(left), BinOp::NullCoalesce, Box::new(right));
        }
        left
    }

    fn parse_or(&mut self) -> Expr {
        let mut left = self.parse_and();
        while self.peek() == &Token::Or {
            self.advance();
            let right = self.parse_and();
            left = Expr::BinOp(Box::new(left), BinOp::Or, Box::new(right));
        }
        left
    }

    fn parse_and(&mut self) -> Expr {
        let mut left = self.parse_bit_or();
        while self.peek() == &Token::And {
            self.advance();
            let right = self.parse_bit_or();
            left = Expr::BinOp(Box::new(left), BinOp::And, Box::new(right));
        }
        left
    }

    fn parse_bit_or(&mut self) -> Expr {
        let mut left = self.parse_bit_xor();
        while self.peek() == &Token::BitOr {
            self.advance();
            let right = self.parse_bit_xor();
            left = Expr::BinOp(Box::new(left), BinOp::BitOr, Box::new(right));
        }
        left
    }

    fn parse_bit_xor(&mut self) -> Expr {
        let mut left = self.parse_bit_and();
        while self.peek() == &Token::BitXor {
            self.advance();
            let right = self.parse_bit_and();
            left = Expr::BinOp(Box::new(left), BinOp::BitXor, Box::new(right));
        }
        left
    }

    fn parse_bit_and(&mut self) -> Expr {
        let mut left = self.parse_equality();
        while self.peek() == &Token::BitAnd {
            self.advance();
            let right = self.parse_equality();
            left = Expr::BinOp(Box::new(left), BinOp::BitAnd, Box::new(right));
        }
        left
    }

    fn parse_equality(&mut self) -> Expr {
        let mut left = self.parse_comparison();
        loop {
            let op = match self.peek() {
                Token::EqEq => BinOp::Eq,
                Token::BangEq => BinOp::Neq,
                Token::EqEqEq => BinOp::StrictEq,
                Token::BangEqEq => BinOp::StrictNeq,
                _ => break,
            };
            self.advance();
            let right = self.parse_comparison();
            left = Expr::BinOp(Box::new(left), op, Box::new(right));
        }
        left
    }

    fn parse_comparison(&mut self) -> Expr {
        let mut left = self.parse_shift();
        loop {
            let op = match self.peek() {
                Token::Lt => BinOp::Lt,
                Token::Gt => BinOp::Gt,
                Token::LtEq => BinOp::Lte,
                Token::GtEq => BinOp::Gte,
                Token::In => BinOp::In,
                _ => break,
            };
            self.advance();
            let right = self.parse_shift();
            left = Expr::BinOp(Box::new(left), op, Box::new(right));
        }
        left
    }

    fn parse_shift(&mut self) -> Expr {
        let mut left = self.parse_additive();
        loop {
            let op = match self.peek() {
                Token::Shl => BinOp::Shl,
                Token::Shr => BinOp::Shr,
                Token::UShr => BinOp::UShr,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive();
            left = Expr::BinOp(Box::new(left), op, Box::new(right));
        }
        left
    }

    fn parse_additive(&mut self) -> Expr {
        let mut left = self.parse_multiplicative();
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative();
            left = Expr::BinOp(Box::new(left), op, Box::new(right));
        }
        left
    }

    fn parse_multiplicative(&mut self) -> Expr {
        let mut left = self.parse_exponent();
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_exponent();
            left = Expr::BinOp(Box::new(left), op, Box::new(right));
        }
        left
    }

    fn parse_exponent(&mut self) -> Expr {
        let base = self.parse_unary();
        if self.peek() == &Token::StarStar {
            self.advance();
            let exp = self.parse_exponent(); // right-associative
            Expr::BinOp(Box::new(base), BinOp::Pow, Box::new(exp))
        } else {
            base
        }
    }

    fn parse_unary(&mut self) -> Expr {
        match self.peek().clone() {
            Token::Minus => {
                self.advance();
                Expr::UnaryOp(UnaryOp::Neg, Box::new(self.parse_unary()))
            }
            Token::Bang => {
                self.advance();
                Expr::UnaryOp(UnaryOp::Not, Box::new(self.parse_unary()))
            }
            Token::BitNot => {
                self.advance();
                Expr::UnaryOp(UnaryOp::BitNot, Box::new(self.parse_unary()))
            }
            Token::Typeof => {
                self.advance();
                Expr::Typeof(Box::new(self.parse_unary()))
            }
            Token::Void => {
                self.advance();
                Expr::Void(Box::new(self.parse_unary()))
            }
            Token::PlusPlus => {
                self.advance();
                Expr::PreIncDec(Box::new(self.parse_unary()), true)
            }
            Token::MinusMinus => {
                self.advance();
                Expr::PreIncDec(Box::new(self.parse_unary()), false)
            }
            Token::New => {
                self.advance();
                let callee = self.parse_primary();
                let args = if self.peek() == &Token::LParen {
                    self.advance();
                    let a = self.parse_arg_list();
                    self.expect(&Token::RParen);
                    a
                } else {
                    vec![]
                };
                Expr::New(Box::new(callee), args)
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Expr {
        let mut expr = self.parse_call_member();
        loop {
            match self.peek() {
                Token::PlusPlus => {
                    self.advance();
                    expr = Expr::PostIncDec(Box::new(expr), true);
                }
                Token::MinusMinus => {
                    self.advance();
                    expr = Expr::PostIncDec(Box::new(expr), false);
                }
                _ => break,
            }
        }
        expr
    }

    fn parse_call_member(&mut self) -> Expr {
        let mut expr = self.parse_primary();
        loop {
            match self.peek() {
                Token::Dot => {
                    self.advance();
                    if let Token::Ident(prop) = self.advance() {
                        expr = Expr::Member(Box::new(expr), prop);
                    }
                }
                Token::LBracket => {
                    self.advance();
                    let idx = self.parse_expr();
                    self.expect(&Token::RBracket);
                    expr = Expr::Index(Box::new(expr), Box::new(idx));
                }
                Token::LParen => {
                    self.advance();
                    let args = self.parse_arg_list();
                    self.expect(&Token::RParen);
                    expr = Expr::Call(Box::new(expr), args);
                }
                _ => break,
            }
        }
        expr
    }

    fn parse_arg_list(&mut self) -> Vec<Expr> {
        let mut args = Vec::new();
        while self.peek() != &Token::RParen && !self.at_end() {
            args.push(self.parse_assignment());
            if self.peek() == &Token::Comma {
                self.advance();
            }
        }
        args
    }

    fn parse_primary(&mut self) -> Expr {
        match self.peek().clone() {
            Token::Number(n) => { self.advance(); Expr::Number(n) }
            Token::Str(s) => { self.advance(); Expr::Str(s) }
            Token::Bool(b) => { self.advance(); Expr::Bool(b) }
            Token::Null => { self.advance(); Expr::Null }
            Token::Undefined => { self.advance(); Expr::Undefined }
            Token::This => { self.advance(); Expr::This }
            Token::Ident(_) => {
                // Check for arrow function: (ident) => or ident =>
                if self.peek_at(1) == &Token::Arrow {
                    if let Token::Ident(name) = self.advance() {
                        self.advance(); // =>
                        let body = if self.peek() == &Token::LBrace {
                            ArrowBody::Block(self.parse_block_stmts())
                        } else {
                            ArrowBody::Expr(self.parse_assignment())
                        };
                        return Expr::Arrow {
                            params: vec![name],
                            body: Box::new(body),
                        };
                    }
                }
                if let Token::Ident(name) = self.advance() {
                    Expr::Ident(name)
                } else {
                    Expr::Undefined
                }
            }
            Token::LParen => {
                // Could be arrow: (a, b) => ...
                // Or grouping: (expr)
                // Try arrow first by looking ahead for ) =>
                if self.is_arrow_params() {
                    self.advance(); // (
                    let params = self.parse_param_list();
                    self.expect(&Token::RParen);
                    self.expect(&Token::Arrow);
                    let body = if self.peek() == &Token::LBrace {
                        ArrowBody::Block(self.parse_block_stmts())
                    } else {
                        ArrowBody::Expr(self.parse_assignment())
                    };
                    Expr::Arrow { params, body: Box::new(body) }
                } else {
                    self.advance(); // (
                    let expr = self.parse_expr();
                    self.expect(&Token::RParen);
                    expr
                }
            }
            Token::LBracket => {
                self.advance();
                let mut elems = Vec::new();
                while self.peek() != &Token::RBracket && !self.at_end() {
                    elems.push(self.parse_assignment());
                    if self.peek() == &Token::Comma {
                        self.advance();
                    }
                }
                self.expect(&Token::RBracket);
                Expr::Array(elems)
            }
            Token::LBrace => {
                self.advance();
                let mut props = Vec::new();
                while self.peek() != &Token::RBrace && !self.at_end() {
                    let key = match self.advance() {
                        Token::Ident(s) | Token::Str(s) => s,
                        Token::Number(n) => format!("{}", n),
                        _ => String::new(),
                    };
                    if self.peek() == &Token::Colon {
                        self.advance();
                        let val = self.parse_assignment();
                        props.push((key, val));
                    } else {
                        // Shorthand: { x } means { x: x }
                        props.push((key.clone(), Expr::Ident(key)));
                    }
                    if self.peek() == &Token::Comma {
                        self.advance();
                    }
                }
                self.expect(&Token::RBrace);
                Expr::Object(props)
            }
            Token::Function => {
                self.advance();
                let name = if let Token::Ident(n) = self.peek().clone() {
                    self.advance();
                    Some(n)
                } else {
                    None
                };
                self.expect(&Token::LParen);
                let params = self.parse_param_list();
                self.expect(&Token::RParen);
                let body = self.parse_block_stmts();
                Expr::FunctionExpr { name, params, body }
            }
            _ => {
                self.advance();
                Expr::Undefined
            }
        }
    }

    fn is_arrow_params(&self) -> bool {
        // Scan ahead from ( to find matching ) =>
        let mut depth = 0;
        let mut i = self.pos;
        while i < self.tokens.len() {
            match &self.tokens[i] {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return i + 1 < self.tokens.len() && self.tokens[i + 1] == Token::Arrow;
                    }
                }
                Token::Eof => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }
}
