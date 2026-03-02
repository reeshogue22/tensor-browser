use std::collections::HashMap;
use std::fmt;

#[derive(Clone)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Object(Object),
    Function(Function),
    NativeFunction(String, fn(&mut Vec<Value>) -> Value),
    Array(Vec<Value>),
}

#[derive(Clone)]
pub struct Object {
    pub props: HashMap<String, Value>,
}

#[derive(Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<String>,
    pub body_start: usize, // index into bytecode
    pub body_len: usize,
}

impl Object {
    pub fn new() -> Self {
        Self { props: HashMap::new() }
    }

    pub fn get(&self, key: &str) -> Value {
        self.props.get(key).cloned().unwrap_or(Value::Undefined)
    }

    pub fn set(&mut self, key: String, val: Value) {
        self.props.insert(key, val);
    }
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::Str(s) => !s.is_empty(),
            Value::Object(_) | Value::Function(_) | Value::NativeFunction(..) | Value::Array(_) => true,
        }
    }

    pub fn to_number(&self) -> f64 {
        match self {
            Value::Undefined => f64::NAN,
            Value::Null => 0.0,
            Value::Bool(b) => if *b { 1.0 } else { 0.0 },
            Value::Number(n) => *n,
            Value::Str(s) => s.parse::<f64>().unwrap_or(f64::NAN),
            _ => f64::NAN,
        }
    }

    pub fn to_string_val(&self) -> String {
        match self {
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
            Value::Str(s) => s.clone(),
            Value::Object(_) => "[object Object]".into(),
            Value::Function(f) => format!("function {}()", f.name),
            Value::NativeFunction(name, _) => format!("function {}() {{ [native] }}", name),
            Value::Array(arr) => {
                let parts: Vec<String> = arr.iter().map(|v| v.to_string_val()).collect();
                parts.join(",")
            }
        }
    }

    pub fn type_of(&self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Null => "object",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::Str(_) => "string",
            Value::Object(_) => "object",
            Value::Function(_) | Value::NativeFunction(..) => "function",
            Value::Array(_) => "object",
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Undefined => write!(f, "undefined"),
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Number(n) => {
                if *n == n.floor() && n.is_finite() && n.abs() < 1e15 {
                    write!(f, "{}", *n as i64)
                } else {
                    write!(f, "{}", n)
                }
            }
            Value::Str(s) => write!(f, "'{}'", s),
            Value::Object(o) => {
                write!(f, "{{")?;
                for (i, (k, v)) in o.props.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}: {:?}", k, v)?;
                }
                write!(f, "}}")
            }
            Value::Function(func) => write!(f, "function {}()", func.name),
            Value::NativeFunction(name, _) => write!(f, "[native: {}]", name),
            Value::Array(arr) => write!(f, "{:?}", arr),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Undefined, Value::Undefined) => true,
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            _ => false,
        }
    }
}
