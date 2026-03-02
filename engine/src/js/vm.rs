use std::collections::HashMap;
use super::lexer::Lexer;
use super::parser::Parser;
use super::compiler::{Compiler, Op};
use super::value::{Value, Object, Function};

struct Scope {
    vars: HashMap<String, Value>,
}

impl Scope {
    fn new() -> Self {
        Self { vars: HashMap::new() }
    }
}

pub struct VM {
    scopes: Vec<Scope>,
    stack: Vec<Value>,
    last_nav: Option<String>,
}

impl VM {
    pub fn new() -> Self {
        let mut vm = Self {
            scopes: vec![Scope::new()],
            stack: Vec::new(),
            last_nav: None,
        };
        vm.init_globals();
        vm
    }

    pub fn eval(&mut self, source: &str) -> Value {
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        let ast = parser.parse();
        let mut compiler = Compiler::new();
        compiler.compile_program(&ast);
        self.execute(&compiler.ops)
    }

    pub fn last_navigation(&self) -> Option<&str> {
        self.last_nav.as_deref()
    }

    fn init_globals(&mut self) {
        // console.log
        let mut console = Object::new();
        console.set("log".into(), Value::NativeFunction("log".into(), |args| {
            let parts: Vec<String> = args.iter().map(|v| v.to_string_val()).collect();
            println!("{}", parts.join(" "));
            Value::Undefined
        }));
        self.set_var("console".into(), Value::Object(console));

        // Math object
        let mut math = Object::new();
        math.set("PI".into(), Value::Number(std::f64::consts::PI));
        math.set("E".into(), Value::Number(std::f64::consts::E));
        math.set("floor".into(), Value::NativeFunction("floor".into(), |args| {
            Value::Number(args.first().map(|v| v.to_number().floor()).unwrap_or(f64::NAN))
        }));
        math.set("ceil".into(), Value::NativeFunction("ceil".into(), |args| {
            Value::Number(args.first().map(|v| v.to_number().ceil()).unwrap_or(f64::NAN))
        }));
        math.set("round".into(), Value::NativeFunction("round".into(), |args| {
            Value::Number(args.first().map(|v| v.to_number().round()).unwrap_or(f64::NAN))
        }));
        math.set("abs".into(), Value::NativeFunction("abs".into(), |args| {
            Value::Number(args.first().map(|v| v.to_number().abs()).unwrap_or(f64::NAN))
        }));
        math.set("sqrt".into(), Value::NativeFunction("sqrt".into(), |args| {
            Value::Number(args.first().map(|v| v.to_number().sqrt()).unwrap_or(f64::NAN))
        }));
        math.set("max".into(), Value::NativeFunction("max".into(), |args| {
            let mut m = f64::NEG_INFINITY;
            for a in args { m = m.max(a.to_number()); }
            Value::Number(m)
        }));
        math.set("min".into(), Value::NativeFunction("min".into(), |args| {
            let mut m = f64::INFINITY;
            for a in args { m = m.min(a.to_number()); }
            Value::Number(m)
        }));
        math.set("random".into(), Value::NativeFunction("random".into(), |_| {
            // Simple LCG random — good enough
            use std::time::SystemTime;
            let seed = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos();
            Value::Number((seed as f64 % 1000.0) / 1000.0)
        }));
        self.set_var("Math".into(), Value::Object(math));

        // JSON.stringify / JSON.parse
        let mut json_obj = Object::new();
        json_obj.set("stringify".into(), Value::NativeFunction("stringify".into(), |args| {
            Value::Str(args.first().map(|v| stringify_value(v)).unwrap_or_default())
        }));
        self.set_var("JSON".into(), Value::Object(json_obj));

        // parseInt / parseFloat
        self.set_var("parseInt".into(), Value::NativeFunction("parseInt".into(), |args| {
            let s = args.first().map(|v| v.to_string_val()).unwrap_or_default();
            let s = s.trim();
            if let Ok(n) = s.parse::<i64>() {
                Value::Number(n as f64)
            } else if s.starts_with("0x") || s.starts_with("0X") {
                Value::Number(i64::from_str_radix(&s[2..], 16).unwrap_or(0) as f64)
            } else {
                Value::Number(f64::NAN)
            }
        }));

        self.set_var("parseFloat".into(), Value::NativeFunction("parseFloat".into(), |args| {
            let s = args.first().map(|v| v.to_string_val()).unwrap_or_default();
            Value::Number(s.trim().parse::<f64>().unwrap_or(f64::NAN))
        }));

        self.set_var("isNaN".into(), Value::NativeFunction("isNaN".into(), |args| {
            Value::Bool(args.first().map(|v| v.to_number().is_nan()).unwrap_or(true))
        }));

        self.set_var("isFinite".into(), Value::NativeFunction("isFinite".into(), |args| {
            Value::Bool(args.first().map(|v| v.to_number().is_finite()).unwrap_or(false))
        }));

        self.set_var("encodeURIComponent".into(), Value::NativeFunction("encodeURIComponent".into(), |args| {
            let s = args.first().map(|v| v.to_string_val()).unwrap_or_default();
            Value::Str(url_encode(&s))
        }));

        self.set_var("decodeURIComponent".into(), Value::NativeFunction("decodeURIComponent".into(), |args| {
            let s = args.first().map(|v| v.to_string_val()).unwrap_or_default();
            Value::Str(url_decode(&s))
        }));

        // String constructor
        self.set_var("String".into(), Value::NativeFunction("String".into(), |args| {
            Value::Str(args.first().map(|v| v.to_string_val()).unwrap_or_default())
        }));

        // Number constructor
        self.set_var("Number".into(), Value::NativeFunction("Number".into(), |args| {
            Value::Number(args.first().map(|v| v.to_number()).unwrap_or(0.0))
        }));

        // Boolean
        self.set_var("Boolean".into(), Value::NativeFunction("Boolean".into(), |args| {
            Value::Bool(args.first().map(|v| v.is_truthy()).unwrap_or(false))
        }));

        // Array.isArray
        let mut array_obj = Object::new();
        array_obj.set("isArray".into(), Value::NativeFunction("isArray".into(), |args| {
            Value::Bool(matches!(args.first(), Some(Value::Array(_))))
        }));
        self.set_var("Array".into(), Value::Object(array_obj));

        // location object — THE key feature
        let mut location = Object::new();
        location.set("href".into(), Value::Str(String::new()));
        location.set("protocol".into(), Value::Str("https:".into()));
        location.set("host".into(), Value::Str(String::new()));
        location.set("pathname".into(), Value::Str("/".into()));
        location.set("search".into(), Value::Str(String::new()));
        location.set("hash".into(), Value::Str(String::new()));
        location.set("origin".into(), Value::Str(String::new()));
        location.set("assign".into(), Value::NativeFunction("assign".into(), |args| {
            if let Some(url) = args.first() {
                // This will be intercepted by the VM
                println!("[navigate] assign: {}", url.to_string_val());
            }
            Value::Undefined
        }));
        location.set("replace".into(), Value::NativeFunction("replace".into(), |args| {
            if let Some(url) = args.first() {
                println!("[navigate] replace: {}", url.to_string_val());
            }
            Value::Undefined
        }));
        location.set("reload".into(), Value::NativeFunction("reload".into(), |_| {
            println!("[navigate] reload");
            Value::Undefined
        }));
        self.set_var("location".into(), Value::Object(location));

        // window = global scope reference
        self.set_var("window".into(), Value::Object(Object::new()));

        // document stub
        let mut document = Object::new();
        document.set("title".into(), Value::Str(String::new()));
        document.set("cookie".into(), Value::Str(String::new()));
        document.set("readyState".into(), Value::Str("complete".into()));
        self.set_var("document".into(), Value::Object(document));

        // navigator stub
        let mut navigator = Object::new();
        navigator.set("userAgent".into(), Value::Str("TensorEngine/0.1".into()));
        navigator.set("language".into(), Value::Str("en-US".into()));
        self.set_var("navigator".into(), Value::Object(navigator));

        self.set_var("undefined".into(), Value::Undefined);
        self.set_var("NaN".into(), Value::Number(f64::NAN));
        self.set_var("Infinity".into(), Value::Number(f64::INFINITY));
    }

    fn get_var(&self, name: &str) -> Value {
        for scope in self.scopes.iter().rev() {
            if let Some(val) = scope.vars.get(name) {
                return val.clone();
            }
        }
        Value::Undefined
    }

    fn set_var(&mut self, name: String, val: Value) {
        // Check location.href assignment — intercept navigation
        if name == "location" {
            if let Value::Str(url) = &val {
                self.last_nav = Some(url.clone());
                return;
            }
        }

        for scope in self.scopes.iter_mut().rev() {
            if scope.vars.contains_key(&name) {
                scope.vars.insert(name, val);
                return;
            }
        }
        // Set in current scope
        if let Some(scope) = self.scopes.last_mut() {
            scope.vars.insert(name, val);
        }
    }

    fn decl_var(&mut self, name: String, val: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.vars.insert(name, val);
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope::new());
    }

    fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    fn execute(&mut self, ops: &[Op]) -> Value {
        let mut ip = 0;
        let mut last_value = Value::Undefined;

        while ip < ops.len() {
            match &ops[ip] {
                Op::Push(val) => {
                    self.stack.push(val.clone());
                }
                Op::Pop => {
                    self.stack.pop();
                }
                Op::Dup => {
                    if let Some(val) = self.stack.last().cloned() {
                        self.stack.push(val);
                    }
                }

                Op::GetVar(name) => {
                    let val = self.get_var(name);
                    self.stack.push(val);
                }
                Op::SetVar(name) => {
                    if let Some(val) = self.stack.last().cloned() {
                        // Intercept location.href = "..."
                        if name == "location" {
                            if let Value::Str(url) = &val {
                                self.last_nav = Some(url.clone());
                            }
                        }
                        self.set_var(name.clone(), val);
                    }
                }
                Op::DeclVar(name) => {
                    let val = self.stack.pop().unwrap_or(Value::Undefined);
                    last_value = val.clone();
                    self.decl_var(name.clone(), val);
                }

                Op::GetProp(prop) => {
                    let obj = self.stack.pop().unwrap_or(Value::Undefined);
                    let val = self.get_property(&obj, prop);
                    self.stack.push(val);
                }
                Op::SetProp(prop) => {
                    let obj_val = self.stack.pop().unwrap_or(Value::Undefined);
                    let val = self.stack.last().cloned().unwrap_or(Value::Undefined);

                    // Intercept location.href = "..."
                    if prop == "href" {
                        // Check if we're setting on the location object
                        if let Value::Object(obj) = &obj_val {
                            if obj.props.contains_key("pathname") && obj.props.contains_key("protocol") {
                                // This is the location object
                                if let Value::Str(url) = &val {
                                    self.last_nav = Some(url.clone());
                                }
                            }
                        }
                    }

                    // Actually set the property
                    self.set_object_property(obj_val, prop.clone(), val);
                }
                Op::GetIndex => {
                    let idx = self.stack.pop().unwrap_or(Value::Undefined);
                    let obj = self.stack.pop().unwrap_or(Value::Undefined);
                    let val = match (&obj, &idx) {
                        (Value::Array(arr), Value::Number(n)) => {
                            arr.get(*n as usize).cloned().unwrap_or(Value::Undefined)
                        }
                        (Value::Object(o), Value::Str(key)) => o.get(key),
                        (Value::Str(s), Value::Number(n)) => {
                            s.chars().nth(*n as usize)
                                .map(|c| Value::Str(c.to_string()))
                                .unwrap_or(Value::Undefined)
                        }
                        _ => Value::Undefined,
                    };
                    self.stack.push(val);
                }
                Op::SetIndex => {
                    let _idx = self.stack.pop();
                    let _obj = self.stack.pop();
                    // Simplified — would need mutable object references
                }

                // Arithmetic
                Op::Add => {
                    let b = self.stack.pop().unwrap_or(Value::Undefined);
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    let result = match (&a, &b) {
                        (Value::Str(s1), _) => Value::Str(format!("{}{}", s1, b.to_string_val())),
                        (_, Value::Str(s2)) => Value::Str(format!("{}{}", a.to_string_val(), s2)),
                        _ => Value::Number(a.to_number() + b.to_number()),
                    };
                    self.stack.push(result);
                }
                Op::Sub => { self.binary_num_op(|a, b| a - b); }
                Op::Mul => { self.binary_num_op(|a, b| a * b); }
                Op::Div => { self.binary_num_op(|a, b| a / b); }
                Op::Mod => { self.binary_num_op(|a, b| a % b); }
                Op::Pow => { self.binary_num_op(|a, b| a.powf(b)); }
                Op::Neg => {
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    self.stack.push(Value::Number(-a.to_number()));
                }

                // Comparison
                Op::Eq | Op::StrictEq => {
                    let b = self.stack.pop().unwrap_or(Value::Undefined);
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    self.stack.push(Value::Bool(a == b));
                }
                Op::Neq | Op::StrictNeq => {
                    let b = self.stack.pop().unwrap_or(Value::Undefined);
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    self.stack.push(Value::Bool(a != b));
                }
                Op::Lt => { self.binary_cmp(|a, b| a < b); }
                Op::Gt => { self.binary_cmp(|a, b| a > b); }
                Op::Lte => { self.binary_cmp(|a, b| a <= b); }
                Op::Gte => { self.binary_cmp(|a, b| a >= b); }

                // Logical
                Op::Not => {
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    self.stack.push(Value::Bool(!a.is_truthy()));
                }
                Op::BitAnd => { self.binary_int_op(|a, b| a & b); }
                Op::BitOr => { self.binary_int_op(|a, b| a | b); }
                Op::BitXor => { self.binary_int_op(|a, b| a ^ b); }
                Op::BitNot => {
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    self.stack.push(Value::Number(!(a.to_number() as i32) as f64));
                }
                Op::Shl => { self.binary_int_op(|a, b| a << (b & 31)); }
                Op::Shr => { self.binary_int_op(|a, b| a >> (b & 31)); }
                Op::UShr => {
                    let b = self.stack.pop().unwrap_or(Value::Undefined);
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    let result = (a.to_number() as u32) >> (b.to_number() as u32 & 31);
                    self.stack.push(Value::Number(result as f64));
                }

                // Control flow
                Op::Jump(target) => {
                    ip = *target;
                    continue;
                }
                Op::JumpIfFalse(target) => {
                    let val = self.stack.pop().unwrap_or(Value::Undefined);
                    if !val.is_truthy() {
                        ip = *target;
                        continue;
                    }
                }
                Op::JumpIfTrue(target) => {
                    let val = self.stack.pop().unwrap_or(Value::Undefined);
                    if val.is_truthy() {
                        ip = *target;
                        continue;
                    }
                }

                // Functions
                Op::Call(argc) => {
                    let argc = *argc;
                    let mut args: Vec<Value> = Vec::new();
                    for _ in 0..argc {
                        args.push(self.stack.pop().unwrap_or(Value::Undefined));
                    }
                    args.reverse();
                    let callee = self.stack.pop().unwrap_or(Value::Undefined);

                    let result = match callee {
                        Value::NativeFunction(_, func) => func(&mut args),
                        Value::Function(func) => {
                            self.call_function(&func, args)
                        }
                        _ => Value::Undefined,
                    };
                    self.stack.push(result);
                }
                Op::Return => {
                    let val = self.stack.pop().unwrap_or(Value::Undefined);
                    return val;
                }
                Op::MakeFunction(name, params, body) => {
                    let func = Value::Function(Function {
                        name: name.clone(),
                        params: params.clone(),
                        body_start: 0,
                        body_len: body.len(),
                    });
                    // Store the bytecode ops alongside the function
                    // We'll use a trick: store ops in the function value
                    self.stack.push(make_closure(name.clone(), params.clone(), body.clone()));
                }
                Op::MakeArrow(params, body) => {
                    self.stack.push(make_closure(String::new(), params.clone(), body.clone()));
                }

                Op::MakeObject(count) => {
                    let count = *count;
                    let mut pairs = Vec::new();
                    for _ in 0..count {
                        let val = self.stack.pop().unwrap_or(Value::Undefined);
                        let key = self.stack.pop().unwrap_or(Value::Undefined);
                        pairs.push((key.to_string_val(), val));
                    }
                    pairs.reverse();
                    let mut obj = Object::new();
                    for (k, v) in pairs {
                        obj.set(k, v);
                    }
                    self.stack.push(Value::Object(obj));
                }
                Op::MakeArray(count) => {
                    let count = *count;
                    let mut elems = Vec::new();
                    for _ in 0..count {
                        elems.push(self.stack.pop().unwrap_or(Value::Undefined));
                    }
                    elems.reverse();
                    self.stack.push(Value::Array(elems));
                }

                Op::Typeof => {
                    let val = self.stack.pop().unwrap_or(Value::Undefined);
                    self.stack.push(Value::Str(val.type_of().into()));
                }
                Op::Void => {
                    self.stack.pop();
                    self.stack.push(Value::Undefined);
                }
                Op::In => {
                    let obj = self.stack.pop().unwrap_or(Value::Undefined);
                    let key = self.stack.pop().unwrap_or(Value::Undefined);
                    let result = match &obj {
                        Value::Object(o) => o.props.contains_key(&key.to_string_val()),
                        _ => false,
                    };
                    self.stack.push(Value::Bool(result));
                }
            }

            // Track last expression value
            if let Some(val) = self.stack.last() {
                last_value = val.clone();
            }

            ip += 1;
        }

        self.stack.pop().unwrap_or(last_value)
    }

    fn call_function(&mut self, func: &Function, args: Vec<Value>) -> Value {
        // Look up closure bytecode
        let ops = CLOSURES.with(|c| {
            let closures = c.borrow();
            closures.get(func.body_start).map(|(_, _, ops)| ops.clone())
        });
        let ops = match ops {
            Some(ops) => ops,
            None => return Value::Undefined,
        };

        // Create new scope with params bound to args
        self.push_scope();
        for (i, param) in func.params.iter().enumerate() {
            let val = args.get(i).cloned().unwrap_or(Value::Undefined);
            self.decl_var(param.clone(), val);
        }
        // arguments object
        self.decl_var("arguments".into(), Value::Array(args));

        let result = self.execute(&ops);
        self.pop_scope();
        result
    }

    fn get_property(&self, obj: &Value, prop: &str) -> Value {
        match obj {
            Value::Object(o) => o.get(prop),
            Value::Str(s) => {
                match prop {
                    "length" => Value::Number(s.len() as f64),
                    "charAt" => Value::NativeFunction("charAt".into(), |args| {
                        // Simplified — would need the string context
                        Value::Str(String::new())
                    }),
                    "indexOf" => Value::NativeFunction("indexOf".into(), |_| Value::Number(-1.0)),
                    "slice" => Value::NativeFunction("slice".into(), |_| Value::Str(String::new())),
                    "split" => Value::NativeFunction("split".into(), |_| Value::Array(vec![])),
                    "trim" => Value::NativeFunction("trim".into(), |_| Value::Str(String::new())),
                    "toLowerCase" => Value::NativeFunction("toLowerCase".into(), |_| Value::Str(String::new())),
                    "toUpperCase" => Value::NativeFunction("toUpperCase".into(), |_| Value::Str(String::new())),
                    "includes" => Value::NativeFunction("includes".into(), |_| Value::Bool(false)),
                    "startsWith" => Value::NativeFunction("startsWith".into(), |_| Value::Bool(false)),
                    "endsWith" => Value::NativeFunction("endsWith".into(), |_| Value::Bool(false)),
                    "replace" => Value::NativeFunction("replace".into(), |_| Value::Str(String::new())),
                    "match" => Value::NativeFunction("match".into(), |_| Value::Null),
                    _ => Value::Undefined,
                }
            }
            Value::Array(arr) => {
                match prop {
                    "length" => Value::Number(arr.len() as f64),
                    "push" => Value::NativeFunction("push".into(), |_| Value::Undefined),
                    "pop" => Value::NativeFunction("pop".into(), |_| Value::Undefined),
                    "map" => Value::NativeFunction("map".into(), |_| Value::Array(vec![])),
                    "filter" => Value::NativeFunction("filter".into(), |_| Value::Array(vec![])),
                    "forEach" => Value::NativeFunction("forEach".into(), |_| Value::Undefined),
                    "join" => Value::NativeFunction("join".into(), |_| Value::Str(String::new())),
                    "indexOf" => Value::NativeFunction("indexOf".into(), |_| Value::Number(-1.0)),
                    "includes" => Value::NativeFunction("includes".into(), |_| Value::Bool(false)),
                    "slice" => Value::NativeFunction("slice".into(), |_| Value::Array(vec![])),
                    "concat" => Value::NativeFunction("concat".into(), |_| Value::Array(vec![])),
                    "reverse" => Value::NativeFunction("reverse".into(), |_| Value::Array(vec![])),
                    "sort" => Value::NativeFunction("sort".into(), |_| Value::Array(vec![])),
                    _ => Value::Undefined,
                }
            }
            _ => Value::Undefined,
        }
    }

    fn set_object_property(&mut self, obj: Value, prop: String, val: Value) {
        // Find the variable that holds this object and mutate it
        // This is simplified — real impl needs object references
        for scope in self.scopes.iter_mut().rev() {
            for (_, v) in scope.vars.iter_mut() {
                if let Value::Object(o) = v {
                    if std::ptr::eq(o as *const _, std::ptr::null()) {
                        continue;
                    }
                    // Check if this is the same object (simplified identity check)
                    o.set(prop.clone(), val.clone());
                    return;
                }
            }
        }
    }

    fn binary_num_op(&mut self, f: fn(f64, f64) -> f64) {
        let b = self.stack.pop().unwrap_or(Value::Undefined);
        let a = self.stack.pop().unwrap_or(Value::Undefined);
        self.stack.push(Value::Number(f(a.to_number(), b.to_number())));
    }

    fn binary_int_op(&mut self, f: fn(i32, i32) -> i32) {
        let b = self.stack.pop().unwrap_or(Value::Undefined);
        let a = self.stack.pop().unwrap_or(Value::Undefined);
        self.stack.push(Value::Number(f(a.to_number() as i32, b.to_number() as i32) as f64));
    }

    fn binary_cmp(&mut self, f: fn(f64, f64) -> bool) {
        let b = self.stack.pop().unwrap_or(Value::Undefined);
        let a = self.stack.pop().unwrap_or(Value::Undefined);
        self.stack.push(Value::Bool(f(a.to_number(), b.to_number())));
    }
}

// Store function bytecode inside the Value using a thread_local hack
// (Real impl would use Rc<Vec<Op>> but we're keeping it dependency-free)
thread_local! {
    static CLOSURES: std::cell::RefCell<Vec<(String, Vec<String>, Vec<Op>)>> = std::cell::RefCell::new(Vec::new());
}

fn make_closure(name: String, params: Vec<String>, ops: Vec<Op>) -> Value {
    let idx = CLOSURES.with(|c| {
        let mut closures = c.borrow_mut();
        let idx = closures.len();
        closures.push((name.clone(), params.clone(), ops));
        idx
    });
    Value::Function(Function {
        name,
        params,
        body_start: idx,
        body_len: 0, // unused, we use body_start as closure index
    })
}

fn stringify_value(val: &Value) -> String {
    match val {
        Value::Undefined => "undefined".into(),
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if *n == n.floor() && n.is_finite() && n.abs() < 1e15 {
                format!("{}", *n as i64)
            } else {
                format!("{}", n)
            }
        }
        Value::Str(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        Value::Object(o) => {
            let parts: Vec<String> = o.props.iter()
                .map(|(k, v)| format!("\"{}\":{}", k, stringify_value(v)))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(|v| stringify_value(v)).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Function(_) | Value::NativeFunction(..) => "null".into(), // JSON spec
    }
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

fn url_decode(s: &str) -> String {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(
                &s[i + 1..i + 3], 16
            ) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into()
}
