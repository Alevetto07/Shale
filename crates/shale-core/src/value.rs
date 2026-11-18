/// A concrete setting value after a formula has been evaluated.
#[derive(Clone, Debug)]
pub enum Value {
    Num(f64),
    Bool(bool),
    Str(String),
    List(Vec<Value>),
    Null,
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.py_eq(other)
    }
}

impl Value {
    pub fn truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(v) => *v,
            Self::Num(n) => *n != 0.0,
            Self::Str(s) => !s.is_empty(),
            Self::List(items) => !items.is_empty(),
        }
    }

    /// Python-style equality: `True == 1`, and numbers compare exactly.
    pub fn py_eq(&self, other: &Value) -> bool {
        match (self.as_number(), other.as_number()) {
            (Some(a), Some(b)) => a == b,
            _ => match (self, other) {
                (Self::Str(a), Self::Str(b)) => a == b,
                (Self::List(a), Self::List(b)) => a == b,
                (Self::Null, Self::Null) => true,
                _ => false,
            },
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Num(n) => Some(*n),
            Self::Bool(true) => Some(1.0),
            Self::Bool(false) => Some(0.0),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }
}
