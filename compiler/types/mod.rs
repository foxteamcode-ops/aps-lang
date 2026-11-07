use crate::compiler::ast::{Capability, TypeAst};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    I8,
    I16,
    I32,
    I64,
    Isize,
    U8,
    U16,
    U32,
    U64,
    Usize,
    F32,
    F64,
    Bool,
    Char,
    Str,
    String,
    Void,
    Never,
    Struct(String, Vec<Type>),
    Enum(String, Vec<Type>),
    Array(Box<Type>, Option<usize>),
    Vec(Box<Type>),
    HashMap(Box<Type>, Box<Type>),
    HashSet(Box<Type>),
    Option(Box<Type>),
    Result(Box<Type>, Box<Type>),
    RegionArena(String),
    RegionBound {
        region: String,
        inner: Box<Type>,
    },
    Shared(Box<Type>),
    Weak(Box<Type>),
    Mutex(Box<Type>),
    Channel(Box<Type>),
    Task(Box<Type>),
    Pointer(Box<Type>),
    Function(Vec<(Capability, Type)>, Box<Type>),
    GenericParam(String),
    Unknown,
    Error,
}

impl Type {
    pub fn is_copy(&self) -> bool {
        match self {
            Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::Isize
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::Usize
            | Type::F32
            | Type::F64
            | Type::Bool
            | Type::Char
            | Type::Void
            | Type::Pointer(_) => true,
            Type::Array(elem, Some(_)) => elem.is_copy(),
            _ => false,
        }
    }

    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Type::I8
                | Type::I16
                | Type::I32
                | Type::I64
                | Type::Isize
                | Type::U8
                | Type::U16
                | Type::U32
                | Type::U64
                | Type::Usize
        )
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Type::F32 | Type::F64)
    }

    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    pub fn strip_region(&self) -> &Type {
        match self {
            Type::RegionBound { inner, .. } => inner.strip_region(),
            other => other,
        }
    }

    pub fn is_compatible_with(&self, other: &Type) -> bool {
        let a = self.strip_region();
        let b = other.strip_region();
        if a == b
            || matches!(a, Type::Unknown | Type::Error | Type::GenericParam(_) | Type::Never)
            || matches!(b, Type::Unknown | Type::Error | Type::GenericParam(_) | Type::Never)
        {
            return true;
        }
        match (a, b) {
            (Type::String, Type::Str) | (Type::Str, Type::String) => true,
            (Type::Struct(n1, _), Type::Struct(n2, _)) => n1 == n2,
            (Type::Enum(n1, _), Type::Enum(n2, _)) => n1 == n2,
            (Type::Array(e1, _), Type::Array(e2, _)) => e1.is_compatible_with(e2),
            (Type::Vec(e1), Type::Vec(e2)) => e1.is_compatible_with(e2),
            (Type::Array(e1, _), Type::Vec(e2)) | (Type::Vec(e1), Type::Array(e2, _)) => {
                e1.is_compatible_with(e2)
            }
            (Type::Option(t1), Type::Option(t2)) => t1.is_compatible_with(t2),
            (Type::Result(o1, e1), Type::Result(o2, e2)) => {
                o1.is_compatible_with(o2) && e1.is_compatible_with(e2)
            }
            (Type::Shared(t1), Type::Shared(t2)) => t1.is_compatible_with(t2),
            (Type::Shared(t1), t2) => t1.is_compatible_with(t2),
            (t1, Type::Shared(t2)) => t1.is_compatible_with(t2),
            (Type::Weak(t1), Type::Weak(t2)) => t1.is_compatible_with(t2),
            (Type::Pointer(t1), Type::Pointer(t2)) => t1.is_compatible_with(t2),
            // Allow integer and float family contextual compatibility while rejecting int<->float without `as`
            (t1, t2) if t1.is_integer() && t2.is_integer() => true,
            (t1, t2) if t1.is_float() && t2.is_float() => true,
            _ => false,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::I8 => write!(f, "i8"),
            Type::I16 => write!(f, "i16"),
            Type::I32 => write!(f, "i32"),
            Type::I64 => write!(f, "i64"),
            Type::Isize => write!(f, "isize"),
            Type::U8 => write!(f, "u8"),
            Type::U16 => write!(f, "u16"),
            Type::U32 => write!(f, "u32"),
            Type::U64 => write!(f, "u64"),
            Type::Usize => write!(f, "usize"),
            Type::F32 => write!(f, "f32"),
            Type::F64 => write!(f, "f64"),
            Type::Bool => write!(f, "bool"),
            Type::Char => write!(f, "char"),
            Type::Str => write!(f, "str"),
            Type::String => write!(f, "String"),
            Type::Void => write!(f, "void"),
            Type::Never => write!(f, "!"),
            Type::Struct(name, args) | Type::Enum(name, args) => {
                if args.is_empty() {
                    write!(f, "{}", name)
                } else {
                    let s: Vec<String> = args.iter().map(|a| a.to_string()).collect();
                    write!(f, "{}<{}>", name, s.join(", "))
                }
            }
            Type::Array(elem, Some(n)) => write!(f, "[{}; {}]", elem, n),
            Type::Array(elem, None) => write!(f, "[{}]", elem),
            Type::Vec(elem) => write!(f, "Vec<{}>", elem),
            Type::HashMap(k, v) => write!(f, "HashMap<{}, {}>", k, v),
            Type::HashSet(t) => write!(f, "HashSet<{}>", t),
            Type::Option(t) => write!(f, "Option<{}>", t),
            Type::Result(t, e) => write!(f, "Result<{}, {}>", t, e),
            Type::RegionArena(r) => write!(f, "Region<{}>", r),
            Type::RegionBound { region, inner } => write!(f, "{}@{}", inner, region),
            Type::Shared(t) => write!(f, "Shared<{}>", t),
            Type::Weak(t) => write!(f, "Weak<{}>", t),
            Type::Mutex(t) => write!(f, "Mutex<{}>", t),
            Type::Channel(t) => write!(f, "Channel<{}>", t),
            Type::Task(t) => write!(f, "Task<{}>", t),
            Type::Pointer(t) => write!(f, "ptr<{}>", t),
            Type::Function(params, ret) => {
                let ps: Vec<String> = params.iter().map(|(_, t)| t.to_string()).collect();
                write!(f, "fn({}) -> {}", ps.join(", "), ret)
            }
            Type::GenericParam(p) => write!(f, "{}", p),
            Type::Unknown => write!(f, "_"),
            Type::Error => write!(f, "<error>"),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct TypeContext {
    pub known_structs: HashMap<String, Vec<(String, Type)>>,
    pub known_enums: HashMap<String, Vec<(String, Vec<Type>)>>,
    pub type_aliases: HashMap<String, Type>,
    pub active_generics: Vec<String>,
}

impl TypeContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn resolve_type_ast(&self, ast: &TypeAst) -> (Capability, Type) {
        match ast {
            TypeAst::Capability { cap, inner } => {
                let (_, ty) = self.resolve_type_ast(inner);
                (*cap, ty)
            }
            TypeAst::Void => (Capability::Value, Type::Void),
            TypeAst::Inferred => (Capability::Value, Type::Unknown),
            TypeAst::Shared(inner) => {
                let (_, ty) = self.resolve_type_ast(inner);
                (Capability::Value, Type::Shared(Box::new(ty)))
            }
            TypeAst::Pointer(inner) => {
                let (_, ty) = self.resolve_type_ast(inner);
                (Capability::Value, Type::Pointer(Box::new(ty)))
            }
            TypeAst::Array { elem, size } => {
                let (_, ty) = self.resolve_type_ast(elem);
                (Capability::Value, Type::Array(Box::new(ty), *size))
            }
            TypeAst::Function { params, ret } => {
                let ps = params.iter().map(|p| self.resolve_type_ast(p)).collect();
                let (_, rt) = self.resolve_type_ast(ret);
                (Capability::Value, Type::Function(ps, Box::new(rt)))
            }
            TypeAst::Named { name, args } => {
                let resolved_args: Vec<Type> =
                    args.iter().map(|a| self.resolve_type_ast(a).1).collect();
                let ty = match name.as_str() {
                    "i8" => Type::I8,
                    "i16" => Type::I16,
                    "i32" => Type::I32,
                    "i64" => Type::I64,
                    "isize" => Type::Isize,
                    "u8" => Type::U8,
                    "u16" => Type::U16,
                    "u32" => Type::U32,
                    "u64" => Type::U64,
                    "usize" => Type::Usize,
                    "f32" => Type::F32,
                    "f64" => Type::F64,
                    "bool" => Type::Bool,
                    "char" => Type::Char,
                    "str" => Type::Str,
                    "String" => Type::String,
                    "void" => Type::Void,
                    "Vec" => Type::Vec(Box::new(
                        resolved_args.into_iter().next().unwrap_or(Type::I64),
                    )),
                    "HashMap" => {
                        let mut it = resolved_args.into_iter();
                        let k = it.next().unwrap_or(Type::String);
                        let v = it.next().unwrap_or(Type::I64);
                        Type::HashMap(Box::new(k), Box::new(v))
                    }
                    "HashSet" => Type::HashSet(Box::new(
                        resolved_args.into_iter().next().unwrap_or(Type::I64),
                    )),
                    "Option" => Type::Option(Box::new(
                        resolved_args.into_iter().next().unwrap_or(Type::I64),
                    )),
                    "Result" => {
                        let mut it = resolved_args.into_iter();
                        let ok = it.next().unwrap_or(Type::I64);
                        let err = it.next().unwrap_or(Type::String);
                        Type::Result(Box::new(ok), Box::new(err))
                    }
                    "Shared" => Type::Shared(Box::new(
                        resolved_args.into_iter().next().unwrap_or(Type::I64),
                    )),
                    "Weak" => Type::Weak(Box::new(
                        resolved_args.into_iter().next().unwrap_or(Type::I64),
                    )),
                    "Mutex" => Type::Mutex(Box::new(
                        resolved_args.into_iter().next().unwrap_or(Type::I64),
                    )),
                    "Channel" => Type::Channel(Box::new(
                        resolved_args.into_iter().next().unwrap_or(Type::I64),
                    )),
                    "Task" => Type::Task(Box::new(
                        resolved_args.into_iter().next().unwrap_or(Type::I64),
                    )),
                    other => {
                        if self.active_generics.iter().any(|g| g == other) {
                            Type::GenericParam(other.to_string())
                        } else if let Some(aliased) = self.type_aliases.get(other) {
                            aliased.clone()
                        } else if self.known_enums.contains_key(other) {
                            Type::Enum(other.to_string(), resolved_args)
                        } else {
                            Type::Struct(other.to_string(), resolved_args)
                        }
                    }
                };
                (Capability::Value, ty)
            }
        }
    }
}
