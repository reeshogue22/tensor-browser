use super::parser::*;
use super::value::Value;

/// Bytecode instructions
#[derive(Debug, Clone)]
pub enum Op {
    // Stack ops
    Push(Value),
    Pop,
    Dup,

    // Variables
    GetVar(String),
    SetVar(String),
    DeclVar(String),

    // Properties
    GetProp(String),
    SetProp(String),
    GetIndex,
    SetIndex,

    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Neg,

    // Comparison
    Eq,
    Neq,
    StrictEq,
    StrictNeq,
    Lt,
    Gt,
    Lte,
    Gte,

    // Logical
    Not,
    BitAnd,
    BitOr,
    BitXor,
    BitNot,
    Shl,
    Shr,
    UShr,

    // Control flow
    Jump(usize),         // absolute jump
    JumpIfFalse(usize),  // jump if top of stack is falsy
    JumpIfTrue(usize),   // jump if top of stack is truthy

    // Functions
    Call(usize),        // number of args
    Return,
    MakeFunction(String, Vec<String>, Vec<Op>),
    MakeArrow(Vec<String>, Vec<Op>),

    // Objects / Arrays
    MakeObject(usize),  // number of key-value pairs (keys on stack before values)
    MakeArray(usize),   // number of elements

    // Special
    Typeof,
    Void,
    In,
}

pub struct Compiler {
    pub ops: Vec<Op>,
}

impl Compiler {
    pub fn new() -> Self {
        Self { ops: Vec::new() }
    }

    pub fn compile_program(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.compile_stmt(stmt);
        }
    }

    fn emit(&mut self, op: Op) -> usize {
        let idx = self.ops.len();
        self.ops.push(op);
        idx
    }

    fn compile_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(expr) => {
                self.compile_expr(expr);
                // Keep last value on stack (for eval result)
            }
            Stmt::VarDecl(name, init) => {
                if let Some(expr) = init {
                    self.compile_expr(expr);
                } else {
                    self.emit(Op::Push(Value::Undefined));
                }
                self.emit(Op::DeclVar(name.clone()));
            }
            Stmt::Block(stmts) => {
                for s in stmts {
                    self.compile_stmt(s);
                }
            }
            Stmt::If(cond, then, else_) => {
                self.compile_expr(cond);
                let jump_false = self.emit(Op::JumpIfFalse(0)); // placeholder
                self.compile_stmt(then);
                if let Some(else_branch) = else_ {
                    let jump_end = self.emit(Op::Jump(0)); // placeholder
                    let else_start = self.ops.len();
                    self.ops[jump_false] = Op::JumpIfFalse(else_start);
                    self.compile_stmt(else_branch);
                    let end = self.ops.len();
                    self.ops[jump_end] = Op::Jump(end);
                } else {
                    let end = self.ops.len();
                    self.ops[jump_false] = Op::JumpIfFalse(end);
                }
            }
            Stmt::While(cond, body) => {
                let loop_start = self.ops.len();
                self.compile_expr(cond);
                let jump_false = self.emit(Op::JumpIfFalse(0));
                self.compile_stmt(body);
                self.emit(Op::Jump(loop_start));
                let end = self.ops.len();
                self.ops[jump_false] = Op::JumpIfFalse(end);
            }
            Stmt::For { init, cond, update, body } => {
                if let Some(init_stmt) = init {
                    self.compile_stmt(init_stmt);
                }
                let loop_start = self.ops.len();
                let jump_false = if let Some(c) = cond {
                    self.compile_expr(c);
                    Some(self.emit(Op::JumpIfFalse(0)))
                } else {
                    None
                };
                self.compile_stmt(body);
                if let Some(upd) = update {
                    self.compile_expr(upd);
                    self.emit(Op::Pop);
                }
                self.emit(Op::Jump(loop_start));
                let end = self.ops.len();
                if let Some(jf) = jump_false {
                    self.ops[jf] = Op::JumpIfFalse(end);
                }
            }
            Stmt::Function { name, params, body } => {
                let mut body_compiler = Compiler::new();
                body_compiler.compile_program(body);
                body_compiler.emit(Op::Push(Value::Undefined));
                body_compiler.emit(Op::Return);
                self.emit(Op::MakeFunction(name.clone(), params.clone(), body_compiler.ops));
                self.emit(Op::DeclVar(name.clone()));
            }
            Stmt::Return(expr) => {
                if let Some(e) = expr {
                    self.compile_expr(e);
                } else {
                    self.emit(Op::Push(Value::Undefined));
                }
                self.emit(Op::Return);
            }
            Stmt::Break => {
                // Simplified: just jump forward (would need a break stack for proper impl)
                self.emit(Op::Push(Value::Undefined));
            }
            Stmt::Continue => {
                self.emit(Op::Push(Value::Undefined));
            }
            Stmt::Throw(expr) => {
                self.compile_expr(expr);
                // For now, just leave on stack
            }
            Stmt::Try { body, catch_body, .. } => {
                // Simplified try/catch — just execute body, if it works fine
                for s in body {
                    self.compile_stmt(s);
                }
                if let Some(cb) = catch_body {
                    // We'd need exception handling infra for real try/catch
                    // For now, catch body is dead code
                    let _ = cb;
                }
            }
            Stmt::Empty => {}
        }
    }

    fn compile_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Number(n) => { self.emit(Op::Push(Value::Number(*n))); }
            Expr::Str(s) => { self.emit(Op::Push(Value::Str(s.clone()))); }
            Expr::Bool(b) => { self.emit(Op::Push(Value::Bool(*b))); }
            Expr::Null => { self.emit(Op::Push(Value::Null)); }
            Expr::Undefined => { self.emit(Op::Push(Value::Undefined)); }
            Expr::This => { self.emit(Op::GetVar("this".into())); }

            Expr::Ident(name) => { self.emit(Op::GetVar(name.clone())); }

            Expr::BinOp(left, op, right) => {
                // Short-circuit for && and ||
                match op {
                    BinOp::And => {
                        self.compile_expr(left);
                        self.emit(Op::Dup);
                        let jump = self.emit(Op::JumpIfFalse(0));
                        self.emit(Op::Pop);
                        self.compile_expr(right);
                        let end = self.ops.len();
                        self.ops[jump] = Op::JumpIfFalse(end);
                        return;
                    }
                    BinOp::Or => {
                        self.compile_expr(left);
                        self.emit(Op::Dup);
                        let jump = self.emit(Op::JumpIfTrue(0));
                        self.emit(Op::Pop);
                        self.compile_expr(right);
                        let end = self.ops.len();
                        self.ops[jump] = Op::JumpIfTrue(end);
                        return;
                    }
                    BinOp::NullCoalesce => {
                        self.compile_expr(left);
                        self.emit(Op::Dup);
                        // If not null/undefined, skip right side
                        let jump = self.emit(Op::JumpIfTrue(0)); // simplified
                        self.emit(Op::Pop);
                        self.compile_expr(right);
                        let end = self.ops.len();
                        self.ops[jump] = Op::JumpIfTrue(end);
                        return;
                    }
                    _ => {}
                }

                self.compile_expr(left);
                self.compile_expr(right);
                match op {
                    BinOp::Add => self.emit(Op::Add),
                    BinOp::Sub => self.emit(Op::Sub),
                    BinOp::Mul => self.emit(Op::Mul),
                    BinOp::Div => self.emit(Op::Div),
                    BinOp::Mod => self.emit(Op::Mod),
                    BinOp::Pow => self.emit(Op::Pow),
                    BinOp::Eq => self.emit(Op::Eq),
                    BinOp::Neq => self.emit(Op::Neq),
                    BinOp::StrictEq => self.emit(Op::StrictEq),
                    BinOp::StrictNeq => self.emit(Op::StrictNeq),
                    BinOp::Lt => self.emit(Op::Lt),
                    BinOp::Gt => self.emit(Op::Gt),
                    BinOp::Lte => self.emit(Op::Lte),
                    BinOp::Gte => self.emit(Op::Gte),
                    BinOp::BitAnd => self.emit(Op::BitAnd),
                    BinOp::BitOr => self.emit(Op::BitOr),
                    BinOp::BitXor => self.emit(Op::BitXor),
                    BinOp::Shl => self.emit(Op::Shl),
                    BinOp::Shr => self.emit(Op::Shr),
                    BinOp::UShr => self.emit(Op::UShr),
                    BinOp::In => self.emit(Op::In),
                    _ => unreachable!(),
                };
            }

            Expr::UnaryOp(op, expr) => {
                self.compile_expr(expr);
                match op {
                    UnaryOp::Neg => self.emit(Op::Neg),
                    UnaryOp::Not => self.emit(Op::Not),
                    UnaryOp::BitNot => self.emit(Op::BitNot),
                    UnaryOp::Typeof => self.emit(Op::Typeof),
                    UnaryOp::Void => self.emit(Op::Void),
                    UnaryOp::Delete => self.emit(Op::Pop), // simplified
                };
            }

            Expr::Assign(target, value) => {
                self.compile_expr(value);
                self.compile_assign_target(target);
            }

            Expr::CompoundAssign(target, op, value) => {
                // Get current value
                self.compile_expr(target);
                self.compile_expr(value);
                match op {
                    BinOp::Add => self.emit(Op::Add),
                    BinOp::Sub => self.emit(Op::Sub),
                    BinOp::Mul => self.emit(Op::Mul),
                    BinOp::Div => self.emit(Op::Div),
                    _ => self.emit(Op::Add),
                };
                self.compile_assign_target(target);
            }

            Expr::Member(obj, prop) => {
                self.compile_expr(obj);
                self.emit(Op::GetProp(prop.clone()));
            }

            Expr::Index(obj, idx) => {
                self.compile_expr(obj);
                self.compile_expr(idx);
                self.emit(Op::GetIndex);
            }

            Expr::Call(callee, args) => {
                // Push callee
                self.compile_expr(callee);
                // Push args
                for arg in args {
                    self.compile_expr(arg);
                }
                self.emit(Op::Call(args.len()));
            }

            Expr::New(callee, args) => {
                self.compile_expr(callee);
                for arg in args {
                    self.compile_expr(arg);
                }
                self.emit(Op::Call(args.len()));
            }

            Expr::Array(elems) => {
                for e in elems {
                    self.compile_expr(e);
                }
                self.emit(Op::MakeArray(elems.len()));
            }

            Expr::Object(props) => {
                for (key, val) in props {
                    self.emit(Op::Push(Value::Str(key.clone())));
                    self.compile_expr(val);
                }
                self.emit(Op::MakeObject(props.len()));
            }

            Expr::FunctionExpr { name, params, body } => {
                let mut body_compiler = Compiler::new();
                body_compiler.compile_program(body);
                body_compiler.emit(Op::Push(Value::Undefined));
                body_compiler.emit(Op::Return);
                self.emit(Op::MakeFunction(
                    name.clone().unwrap_or_default(),
                    params.clone(),
                    body_compiler.ops,
                ));
            }

            Expr::Arrow { params, body } => {
                let mut body_compiler = Compiler::new();
                match body.as_ref() {
                    ArrowBody::Expr(expr) => {
                        body_compiler.compile_expr(expr);
                        body_compiler.emit(Op::Return);
                    }
                    ArrowBody::Block(stmts) => {
                        body_compiler.compile_program(stmts);
                        body_compiler.emit(Op::Push(Value::Undefined));
                        body_compiler.emit(Op::Return);
                    }
                }
                self.emit(Op::MakeArrow(params.clone(), body_compiler.ops));
            }

            Expr::Ternary(cond, then, else_) => {
                self.compile_expr(cond);
                let jump_false = self.emit(Op::JumpIfFalse(0));
                self.compile_expr(then);
                let jump_end = self.emit(Op::Jump(0));
                let else_start = self.ops.len();
                self.ops[jump_false] = Op::JumpIfFalse(else_start);
                self.compile_expr(else_);
                let end = self.ops.len();
                self.ops[jump_end] = Op::Jump(end);
            }

            Expr::Typeof(expr) => {
                self.compile_expr(expr);
                self.emit(Op::Typeof);
            }

            Expr::Void(expr) => {
                self.compile_expr(expr);
                self.emit(Op::Void);
            }

            Expr::PreIncDec(expr, is_inc) => {
                self.compile_expr(expr);
                self.emit(Op::Push(Value::Number(1.0)));
                if *is_inc { self.emit(Op::Add); } else { self.emit(Op::Sub); }
                self.compile_assign_target(expr);
            }

            Expr::PostIncDec(expr, is_inc) => {
                self.compile_expr(expr);
                self.emit(Op::Dup); // keep old value
                self.emit(Op::Push(Value::Number(1.0)));
                if *is_inc { self.emit(Op::Add); } else { self.emit(Op::Sub); }
                self.compile_assign_target(expr);
                self.emit(Op::Pop); // remove new value, old value stays
            }
        }
    }

    fn compile_assign_target(&mut self, target: &Expr) {
        match target {
            Expr::Ident(name) => { self.emit(Op::SetVar(name.clone())); }
            Expr::Member(obj, prop) => {
                self.compile_expr(obj);
                self.emit(Op::SetProp(prop.clone()));
            }
            Expr::Index(obj, idx) => {
                self.compile_expr(obj);
                self.compile_expr(idx);
                self.emit(Op::SetIndex);
            }
            _ => {} // invalid assignment target
        }
    }
}
