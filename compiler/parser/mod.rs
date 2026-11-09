use crate::compiler::ast::*;
use crate::compiler::diagnostics::{Diagnostic, DiagnosticBag, Span};
use crate::compiler::lexer::{Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    allow_struct_lit: bool,
    pub diagnostics: DiagnosticBag,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            allow_struct_lit: true,
            diagnostics: DiagnosticBag::new(),
        }
    }

    fn peek(&self) -> &Token {
        self.tokens
            .get(self.pos)
            .unwrap_or_else(|| self.tokens.last().unwrap())
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    fn peek_next_kind(&self) -> Option<&TokenKind> {
        self.tokens.get(self.pos + 1).map(|t| &t.kind)
    }

    fn span(&self) -> Span {
        self.peek().span.clone()
    }

    fn is_eof(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eof)
    }

    fn advance(&mut self) -> Token {
        if !self.is_eof() {
            let tok = self.tokens[self.pos].clone();
            self.pos += 1;
            tok
        } else {
            self.tokens.last().unwrap().clone()
        }
    }

    fn eat(&mut self, expected: &TokenKind) -> bool {
        if self.peek_kind() == expected {
            self.advance();
            true
        } else {
            false
        }
    }

    fn eat_optional_semi(&mut self) -> bool {
        let mut ate = false;
        while self.eat(&TokenKind::Semi) {
            ate = true;
        }
        ate
    }

    fn expect(&mut self, expected: &TokenKind, desc: &str) -> Span {
        if self.peek_kind() == expected {
            self.advance().span
        } else {
            let sp = self.span();
            self.diagnostics.push(
                Diagnostic::error(
                    "E0010",
                    format!("expected {}, found `{:?}`", desc, self.peek_kind()),
                    sp.clone(),
                )
                .with_help(format!("insert {}", desc)),
            );
            sp
        }
    }

    fn expect_ident(&mut self, desc: &str) -> (String, Span) {
        let sp = self.span();
        match self.peek_kind().clone() {
            TokenKind::Ident(name) => {
                self.advance();
                (name, sp)
            }
            // Allow some contextual keywords as identifiers in field/method positions
            TokenKind::Read => {
                self.advance();
                ("read".to_string(), sp)
            }
            TokenKind::Write => {
                self.advance();
                ("write".to_string(), sp)
            }
            TokenKind::Take => {
                self.advance();
                ("take".to_string(), sp)
            }
            TokenKind::Share => {
                self.advance();
                ("share".to_string(), sp)
            }
            TokenKind::Type => {
                self.advance();
                ("type".to_string(), sp)
            }
            TokenKind::Async => {
                self.advance();
                ("async".to_string(), sp)
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    "E0011",
                    format!("expected identifier for {}, found `{:?}`", desc, self.peek_kind()),
                    sp.clone(),
                ));
                self.advance();
                ("_error".to_string(), sp)
            }
        }
    }

    pub fn parse_program(&mut self) -> Program {
        let mut module_name = None;
        let mut imports = Vec::new();
        let mut items = Vec::new();

        self.eat_optional_semi();

        if self.eat(&TokenKind::Module) {
            module_name = Some(self.parse_module_path());
            self.eat_optional_semi();
        }

        while self.eat(&TokenKind::Import) {
            let span = self.span();
            let path = self.parse_module_path();
            let alias = if self.eat(&TokenKind::As) {
                Some(self.expect_ident("import alias").0)
            } else {
                None
            };
            self.eat_optional_semi();
            imports.push(ImportDecl { path, alias, span });
        }

        while !self.is_eof() {
            if self.eat(&TokenKind::Import) {
                let span = self.span();
                let path = self.parse_module_path();
                let alias = if self.eat(&TokenKind::As) {
                    Some(self.expect_ident("import alias").0)
                } else {
                    None
                };
                self.eat_optional_semi();
                imports.push(ImportDecl { path, alias, span });
                continue;
            }
            if self.eat(&TokenKind::Module) {
                let _ = self.parse_module_path();
                if self.eat(&TokenKind::LBrace) {
                    while !self.is_eof() && !self.eat(&TokenKind::RBrace) {
                        if let Some(item) = self.parse_item() {
                            items.push(item);
                        }
                    }
                }
                self.eat_optional_semi();
                continue;
            }
            if let Some(item) = self.parse_item() {
                items.push(item);
            } else {
                self.advance();
            }
            self.eat_optional_semi();
        }

        Program {
            module_name,
            imports,
            items,
        }
    }

    fn parse_module_path(&mut self) -> Vec<String> {
        let mut path = Vec::new();
        let (first, _) = self.expect_ident("module name");
        path.push(first);
        while self.eat(&TokenKind::Dot) || self.eat(&TokenKind::ColonColon) {
            let (seg, _) = self.expect_ident("module path segment");
            path.push(seg);
        }
        path
    }

    fn parse_attributes(&mut self) -> Vec<String> {
        let mut attrs = Vec::new();
        while self.eat(&TokenKind::At) {
            let (name, _) = self.expect_ident("attribute name");
            let mut attr_str = name;
            if self.eat(&TokenKind::LParen) {
                attr_str.push('(');
                while !self.is_eof() && !self.eat(&TokenKind::RParen) {
                    match self.advance().kind {
                        TokenKind::Ident(s) => attr_str.push_str(&s),
                        TokenKind::IntLit(n, _) => attr_str.push_str(&n.to_string()),
                        TokenKind::Comma => attr_str.push(','),
                        _ => {}
                    }
                }
                attr_str.push(')');
            }
            attrs.push(attr_str);
        }
        attrs
    }

    fn parse_item(&mut self) -> Option<Item> {
        let attrs = self.parse_attributes();
        let is_pub = self.eat(&TokenKind::Pub);
        let is_async = self.eat(&TokenKind::Async);
        let is_const_fn = if self.peek_kind() == &TokenKind::Const
            && self.peek_next_kind() == Some(&TokenKind::Fn)
        {
            self.advance();
            true
        } else {
            false
        };

        match self.peek_kind() {
            TokenKind::Fn => Some(Item::Function(self.parse_fn_decl(
                is_pub,
                is_async,
                is_const_fn,
            ))),
            TokenKind::Struct => Some(Item::Struct(self.parse_struct_decl(is_pub, attrs))),
            TokenKind::Enum => Some(Item::Enum(self.parse_enum_decl(is_pub))),
            TokenKind::Const => Some(Item::Const(self.parse_const_decl(is_pub))),
            TokenKind::Impl => Some(Item::Impl(self.parse_impl_block())),
            TokenKind::Trait => Some(Item::Trait(self.parse_trait_decl(is_pub))),
            TokenKind::Extern => Some(Item::Extern(self.parse_extern_block())),
            TokenKind::Type => {
                let span = self.advance().span;
                let (name, _) = self.expect_ident("type alias name");
                let _ = self.parse_generics();
                self.expect(&TokenKind::Eq, "`=`");
                let ty = self.parse_type();
                self.eat_optional_semi();
                Some(Item::TypeAlias {
                    is_pub,
                    name,
                    ty,
                    span,
                })
            }
            _ => {
                let sp = self.span();
                self.diagnostics.push(Diagnostic::error(
                    "E0012",
                    format!("unexpected top-level token `{:?}`", self.peek_kind()),
                    sp,
                ));
                None
            }
        }
    }

    fn parse_generics(&mut self) -> Vec<GenericParam> {
        let mut params = Vec::new();
        if self.eat(&TokenKind::Lt) {
            let mut closed = false;
            while !self.is_eof() {
                if self.eat(&TokenKind::Gt) {
                    closed = true;
                    break;
                }
                let (name, _) = self.expect_ident("generic parameter");
                if name == "_error" {
                    break;
                }
                let bound = if self.eat(&TokenKind::Colon) {
                    Some(self.expect_ident("trait bound").0)
                } else {
                    None
                };
                params.push(GenericParam { name, bound });
                if self.eat(&TokenKind::Gt) {
                    closed = true;
                    break;
                }
                if self.peek_kind() == &TokenKind::Shr {
                    self.tokens[self.pos].kind = TokenKind::Gt;
                    closed = true;
                    break;
                }
                if !self.eat(&TokenKind::Comma) {
                    if self.eat(&TokenKind::Gt) {
                        closed = true;
                    } else if self.peek_kind() == &TokenKind::Shr {
                        self.tokens[self.pos].kind = TokenKind::Gt;
                        closed = true;
                    }
                    break;
                }
            }
            if !closed {
                return Vec::new();
            }
        }
        params
    }

    fn parse_capability(&mut self) -> Capability {
        if self.eat(&TokenKind::Read) {
            Capability::Read
        } else if self.eat(&TokenKind::Write) {
            Capability::Write
        } else if self.eat(&TokenKind::Take) {
            Capability::Take
        } else {
            Capability::Value
        }
    }

    fn parse_type(&mut self) -> TypeAst {
        let cap = self.parse_capability();
        let base = self.parse_base_type();
        if cap != Capability::Value {
            TypeAst::Capability {
                cap,
                inner: Box::new(base),
            }
        } else {
            base
        }
    }

    fn parse_base_type(&mut self) -> TypeAst {
        if self.eat(&TokenKind::Shared) {
            return TypeAst::Shared(Box::new(self.parse_base_type()));
        }
        if self.eat(&TokenKind::Fn) {
            self.expect(&TokenKind::LParen, "`(`");
            let mut params = Vec::new();
            while !self.is_eof() && !self.eat(&TokenKind::RParen) {
                params.push(self.parse_type());
                if !self.eat(&TokenKind::Comma) {
                    let _ = self.eat(&TokenKind::RParen);
                    break;
                }
            }
            let ret = if self.eat(&TokenKind::Arrow) {
                self.parse_type()
            } else {
                TypeAst::Void
            };
            return TypeAst::Function {
                params,
                ret: Box::new(ret),
            };
        }
        if self.eat(&TokenKind::LBracket) {
            let elem = self.parse_type();
            let size = if self.eat(&TokenKind::Semi) {
                if let TokenKind::IntLit(n, _) = self.peek_kind().clone() {
                    self.advance();
                    Some(n as usize)
                } else {
                    None
                }
            } else {
                None
            };
            self.expect(&TokenKind::RBracket, "`]`");
            return TypeAst::Array {
                elem: Box::new(elem),
                size,
            };
        }
        if self.eat(&TokenKind::LParen) {
            self.expect(&TokenKind::RParen, "`)`");
            return TypeAst::Void;
        }

        let (mut name, _) = self.expect_ident("type name");
        while self.eat(&TokenKind::Dot) || self.eat(&TokenKind::ColonColon) {
            let (seg, _) = self.expect_ident("type segment");
            name = seg;
        }
        if name == "void" {
            return TypeAst::Void;
        }

        let mut args = Vec::new();
        if self.eat(&TokenKind::Lt) {
            while !self.is_eof() && !self.eat(&TokenKind::Gt) {
                args.push(self.parse_type());
                if !self.eat(&TokenKind::Comma) {
                    // Handle nested `>>` token if needed
                    if self.peek_kind() == &TokenKind::Shr {
                        self.tokens[self.pos].kind = TokenKind::Gt;
                        break;
                    }
                    let _ = self.eat(&TokenKind::Gt);
                    break;
                }
            }
        }

        if name == "ptr" && args.len() == 1 {
            return TypeAst::Pointer(Box::new(args.remove(0)));
        }
        if (name == "Shared" || name == "shared") && args.len() == 1 {
            return TypeAst::Shared(Box::new(args.remove(0)));
        }

        let is_builtin = matches!(
            name.as_str(),
            "Vec"
                | "HashMap"
                | "HashSet"
                | "Option"
                | "Result"
                | "Channel"
                | "channel"
                | "Receiver"
                | "Sender"
                | "Task"
                | "Shared"
                | "Weak"
                | "Mutex"
                | "spawn_thread"
                | "block_on"
                | "promote"
        );
        if !is_builtin && !args.is_empty() {
            let mut arg_names = Vec::new();
            for a in &args {
                if let TypeAst::Named { name: aname, args: sub_args } = a {
                    if sub_args.is_empty() {
                        arg_names.push(aname.clone());
                    }
                }
            }
            if arg_names.len() == args.len() {
                return TypeAst::Named {
                    name: format!("{}${}", name, arg_names.join("$")),
                    args: Vec::new(),
                };
            }
        }

        TypeAst::Named { name, args }
    }

    fn parse_params(&mut self) -> Vec<Param> {
        let mut params = Vec::new();
        self.expect(&TokenKind::LParen, "`(`");
        while !self.is_eof() && !self.eat(&TokenKind::RParen) {
            let span = self.span();
            let is_var = self.eat(&TokenKind::Var);
            let cap_before = self.parse_capability();
            let (name, _) = self.expect_ident("parameter name");
            let (capability, ty) = if self.eat(&TokenKind::Colon) {
                let cap_after = self.parse_capability();
                let base_ty = self.parse_base_type();
                let final_cap = if cap_after != Capability::Value {
                    cap_after
                } else {
                    cap_before
                };
                (final_cap, base_ty)
            } else {
                (
                    cap_before,
                    TypeAst::Named {
                        name: "Self".to_string(),
                        args: Vec::new(),
                    },
                )
            };
            params.push(Param {
                name,
                capability,
                ty,
                is_mutable: is_var || capability == Capability::Write,
                span,
            });
            if !self.eat(&TokenKind::Comma) {
                let _ = self.eat(&TokenKind::RParen);
                break;
            }
        }
        params
    }

    fn parse_fn_decl(&mut self, is_pub: bool, is_async: bool, is_const: bool) -> FnDecl {
        let span = self.expect(&TokenKind::Fn, "`fn`");
        let (name, _) = self.expect_ident("function name");
        let generics = self.parse_generics();
        let params = self.parse_params();
        let ret_type = if self.eat(&TokenKind::Arrow) {
            self.parse_type()
        } else {
            TypeAst::Void
        };
        let body = self.parse_block();
        FnDecl {
            is_pub,
            is_async,
            is_const,
            name,
            generics,
            params,
            ret_type,
            body,
            span,
        }
    }

    fn parse_struct_decl(&mut self, is_pub: bool, attributes: Vec<String>) -> StructDecl {
        let span = self.expect(&TokenKind::Struct, "`struct`");
        let (name, _) = self.expect_ident("struct name");
        let generics = self.parse_generics();
        self.expect(&TokenKind::LBrace, "`{`");
        let mut fields = Vec::new();
        let mut closed = false;
        while !self.is_eof() {
            if self.eat(&TokenKind::RBrace) {
                closed = true;
                break;
            }
            let f_span = self.span();
            let f_pub = self.eat(&TokenKind::Pub);
            let (f_name, _) = self.expect_ident("field name");
            self.expect(&TokenKind::Colon, "`:`");
            let f_ty = self.parse_type();
            let _ = self.eat(&TokenKind::Comma) || self.eat(&TokenKind::Semi);
            fields.push(StructField {
                is_pub: f_pub,
                name: f_name,
                ty: f_ty,
                span: f_span,
            });
        }
        if !closed {
            self.expect(&TokenKind::RBrace, "`}`");
        }
        StructDecl {
            is_pub,
            name,
            generics,
            fields,
            attributes,
            span,
        }
    }

    fn parse_enum_decl(&mut self, is_pub: bool) -> EnumDecl {
        let span = self.expect(&TokenKind::Enum, "`enum`");
        let (name, _) = self.expect_ident("enum name");
        let generics = self.parse_generics();
        self.expect(&TokenKind::LBrace, "`{`");
        let mut variants = Vec::new();
        let mut closed = false;
        while !self.is_eof() {
            if self.eat(&TokenKind::RBrace) {
                closed = true;
                break;
            }
            let v_span = self.span();
            let (v_name, _) = self.expect_ident("enum variant name");
            let kind = if self.eat(&TokenKind::LParen) {
                let mut tuple_tys = Vec::new();
                while !self.is_eof() && !self.eat(&TokenKind::RParen) {
                    tuple_tys.push(self.parse_type());
                    if !self.eat(&TokenKind::Comma) {
                        let _ = self.eat(&TokenKind::RParen);
                        break;
                    }
                }
                EnumVariantKind::Tuple(tuple_tys)
            } else if self.eat(&TokenKind::LBrace) {
                let mut s_fields = Vec::new();
                while !self.is_eof() && !self.eat(&TokenKind::RBrace) {
                    let fs = self.span();
                    let fp = self.eat(&TokenKind::Pub);
                    let (fnm, _) = self.expect_ident("variant field name");
                    self.expect(&TokenKind::Colon, "`:`");
                    let fty = self.parse_type();
                    let _ = self.eat(&TokenKind::Comma) || self.eat(&TokenKind::Semi);
                    s_fields.push(StructField {
                        is_pub: fp,
                        name: fnm,
                        ty: fty,
                        span: fs,
                    });
                }
                EnumVariantKind::Struct(s_fields)
            } else {
                EnumVariantKind::Unit
            };
            let _ = self.eat(&TokenKind::Comma) || self.eat(&TokenKind::Semi);
            variants.push(EnumVariant {
                name: v_name,
                kind,
                span: v_span,
            });
        }
        if !closed {
            self.expect(&TokenKind::RBrace, "`}`");
        }
        EnumDecl {
            is_pub,
            name,
            generics,
            variants,
            span,
        }
    }

    fn parse_impl_block(&mut self) -> ImplBlock {
        let span = self.expect(&TokenKind::Impl, "`impl`");
        let generics = self.parse_generics();
        let (first_name, _) = self.expect_ident("impl target");
        let _ = self.parse_generics();
        let (target_name, trait_name) = if self.eat(&TokenKind::For) {
            let (tgt, _) = self.expect_ident("target type");
            let _ = self.parse_generics();
            (tgt, Some(first_name))
        } else {
            (first_name, None)
        };
        self.expect(&TokenKind::LBrace, "`{`");
        let mut methods = Vec::new();
        let mut closed = false;
        while !self.is_eof() {
            if self.eat(&TokenKind::RBrace) {
                closed = true;
                break;
            }
            let _ = self.parse_attributes();
            let is_pub = self.eat(&TokenKind::Pub);
            let is_async = self.eat(&TokenKind::Async);
            let is_const = self.eat(&TokenKind::Const);
            if self.peek_kind() == &TokenKind::Fn {
                methods.push(self.parse_fn_decl(is_pub, is_async, is_const));
            } else {
                self.advance();
            }
        }
        if !closed {
            self.expect(&TokenKind::RBrace, "`}`");
        }
        ImplBlock {
            generics,
            target_name,
            trait_name,
            methods,
            span,
        }
    }

    fn parse_trait_decl(&mut self, is_pub: bool) -> TraitDecl {
        let span = self.expect(&TokenKind::Trait, "`trait`");
        let (name, _) = self.expect_ident("trait name");
        let generics = self.parse_generics();
        self.expect(&TokenKind::LBrace, "`{`");
        let mut methods = Vec::new();
        let mut closed = false;
        while !self.is_eof() {
            if self.eat(&TokenKind::RBrace) {
                closed = true;
                break;
            }
            let m_span = self.expect(&TokenKind::Fn, "`fn`");
            let (m_name, _) = self.expect_ident("trait method name");
            let m_gen = self.parse_generics();
            let params = self.parse_params();
            let ret_type = if self.eat(&TokenKind::Arrow) {
                self.parse_type()
            } else {
                TypeAst::Void
            };
            if self.peek_kind() == &TokenKind::LBrace {
                let _ = self.parse_block();
            }
            self.eat_optional_semi();
            methods.push(FnSig {
                name: m_name,
                generics: m_gen,
                params,
                ret_type,
                span: m_span,
            });
        }
        if !closed {
            self.expect(&TokenKind::RBrace, "`}`");
        }
        TraitDecl {
            is_pub,
            name,
            generics,
            methods,
            span,
        }
    }

    fn parse_extern_block(&mut self) -> ExternBlock {
        let span = self.expect(&TokenKind::Extern, "`extern`");
        let abi = if let TokenKind::StringLit(s) = self.peek_kind().clone() {
            self.advance();
            s
        } else {
            "C".to_string()
        };
        if self.peek_kind() == &TokenKind::Fn {
            let m_span = self.expect(&TokenKind::Fn, "`fn`");
            let (m_name, _) = self.expect_ident("extern function name");
            let m_gen = self.parse_generics();
            let params = self.parse_params();
            let ret_type = if self.eat(&TokenKind::Arrow) {
                self.parse_type()
            } else {
                TypeAst::Void
            };
            self.eat_optional_semi();
            return ExternBlock {
                abi,
                functions: vec![FnSig {
                    name: m_name,
                    generics: m_gen,
                    params,
                    ret_type,
                    span: m_span,
                }],
                span,
            };
        }
        self.expect(&TokenKind::LBrace, "`{`");
        let mut functions = Vec::new();
        let mut closed = false;
        while !self.is_eof() {
            if self.eat(&TokenKind::RBrace) {
                closed = true;
                break;
            }
            let _ = self.eat(&TokenKind::Pub);
            let m_span = self.expect(&TokenKind::Fn, "`fn`");
            let (m_name, _) = self.expect_ident("extern function name");
            let m_gen = self.parse_generics();
            let params = self.parse_params();
            let ret_type = if self.eat(&TokenKind::Arrow) {
                self.parse_type()
            } else {
                TypeAst::Void
            };
            self.eat_optional_semi();
            functions.push(FnSig {
                name: m_name,
                generics: m_gen,
                params,
                ret_type,
                span: m_span,
            });
        }
        if !closed {
            self.expect(&TokenKind::RBrace, "`}`");
        }
        ExternBlock {
            abi,
            functions,
            span,
        }
    }

    fn parse_const_decl(&mut self, is_pub: bool) -> ConstDecl {
        let span = self.expect(&TokenKind::Const, "`const`");
        let (name, _) = self.expect_ident("constant name");
        let ty = if self.eat(&TokenKind::Colon) {
            self.parse_type()
        } else {
            TypeAst::Inferred
        };
        self.expect(&TokenKind::Eq, "`=`");
        let value = self.parse_expr();
        self.eat_optional_semi();
        ConstDecl {
            is_pub,
            name,
            ty,
            value,
            span,
        }
    }

    fn parse_block(&mut self) -> Block {
        let span = self.expect(&TokenKind::LBrace, "`{`");
        let mut stmts = Vec::new();
        self.eat_optional_semi();
        let mut closed = false;
        while !self.is_eof() {
            if self.eat(&TokenKind::RBrace) {
                closed = true;
                break;
            }
            let stmt = self.parse_stmt();
            stmts.push(stmt);
            self.eat_optional_semi();
        }
        if !closed {
            self.expect(&TokenKind::RBrace, "`}`");
        }
        Block { stmts, span }
    }

    fn parse_stmt(&mut self) -> Stmt {
        let span = self.span();
        match self.peek_kind() {
            TokenKind::Let => {
                self.advance();
                let (name, _) = self.expect_ident("variable name");
                let ty = if self.eat(&TokenKind::Colon) {
                    self.parse_type()
                } else {
                    TypeAst::Inferred
                };
                self.expect(&TokenKind::Eq, "`=`");
                let init = self.parse_expr();
                self.eat_optional_semi();
                Stmt::Let {
                    name,
                    ty,
                    init,
                    span,
                }
            }
            TokenKind::Var => {
                self.advance();
                let (name, _) = self.expect_ident("mutable variable name");
                let ty = if self.eat(&TokenKind::Colon) {
                    self.parse_type()
                } else {
                    TypeAst::Inferred
                };
                self.expect(&TokenKind::Eq, "`=`");
                let init = self.parse_expr();
                self.eat_optional_semi();
                Stmt::Var {
                    name,
                    ty,
                    init,
                    span,
                }
            }
            TokenKind::Const => Stmt::Const(self.parse_const_decl(false)),
            TokenKind::Return => {
                self.advance();
                let value = if matches!(self.peek_kind(), TokenKind::Semi | TokenKind::RBrace | TokenKind::Eof)
                    || self.peek().preceded_by_newline
                {
                    None
                } else {
                    Some(self.parse_expr())
                };
                self.eat_optional_semi();
                Stmt::Return { value, span }
            }
            TokenKind::Break => {
                self.advance();
                self.eat_optional_semi();
                Stmt::Break { span }
            }
            TokenKind::Continue => {
                self.advance();
                self.eat_optional_semi();
                Stmt::Continue { span }
            }
            TokenKind::Defer => {
                self.advance();
                let body = if self.peek_kind() == &TokenKind::LBrace {
                    self.parse_block()
                } else {
                    let e = self.parse_expr();
                    let es = e.span.clone();
                    Block {
                        stmts: vec![Stmt::Expr {
                            expr: e,
                            has_semi: true,
                            span: es.clone(),
                        }],
                        span: es,
                    }
                };
                self.eat_optional_semi();
                Stmt::Defer { body, span }
            }
            TokenKind::Region => {
                self.advance();
                let (name, _) = self.expect_ident("region name");
                let body = self.parse_block();
                self.eat_optional_semi();
                Stmt::Region { name, body, span }
            }
            TokenKind::Unsafe => {
                self.advance();
                let body = self.parse_block();
                self.eat_optional_semi();
                Stmt::Unsafe { body, span }
            }
            TokenKind::While => {
                self.advance();
                let prev = self.allow_struct_lit;
                self.allow_struct_lit = false;
                let cond = self.parse_expr();
                self.allow_struct_lit = prev;
                let body = self.parse_block();
                Stmt::While { cond, body, span }
            }
            TokenKind::For => {
                self.advance();
                let (var, _) = self.expect_ident("loop variable");
                self.expect(&TokenKind::In, "`in`");
                let prev = self.allow_struct_lit;
                self.allow_struct_lit = false;
                let iter = self.parse_expr();
                self.allow_struct_lit = prev;
                let body = self.parse_block();
                Stmt::For {
                    var,
                    iter,
                    body,
                    span,
                }
            }
            TokenKind::Loop => {
                self.advance();
                let body = self.parse_block();
                Stmt::Loop { body, span }
            }
            _ => {
                let expr = self.parse_expr();
                let assign_op = match self.peek_kind() {
                    TokenKind::Eq => Some(AssignOp::Assign),
                    TokenKind::PlusEq => Some(AssignOp::AddAssign),
                    TokenKind::MinusEq => Some(AssignOp::SubAssign),
                    TokenKind::StarEq => Some(AssignOp::MulAssign),
                    TokenKind::SlashEq => Some(AssignOp::DivAssign),
                    TokenKind::PercentEq => Some(AssignOp::RemAssign),
                    _ => None,
                };
                if let Some(op) = assign_op {
                    self.advance();
                    let value = self.parse_expr();
                    self.eat_optional_semi();
                    Stmt::Assign {
                        target: expr,
                        op,
                        value,
                        span,
                    }
                } else {
                    let has_semi = self.eat_optional_semi();
                    Stmt::Expr {
                        expr,
                        has_semi,
                        span,
                    }
                }
            }
        }
    }

    pub fn parse_expr(&mut self) -> Expr {
        self.parse_prec_expr(1)
    }

    fn binop_prec(kind: &TokenKind) -> Option<(u8, Option<BinOp>)> {
        match kind {
            TokenKind::OrOr => Some((1, Some(BinOp::Or))),
            TokenKind::AndAnd => Some((2, Some(BinOp::And))),
            TokenKind::EqEq => Some((3, Some(BinOp::Eq))),
            TokenKind::NotEq => Some((3, Some(BinOp::Ne))),
            TokenKind::Lt => Some((3, Some(BinOp::Lt))),
            TokenKind::LtEq => Some((3, Some(BinOp::Le))),
            TokenKind::Gt => Some((3, Some(BinOp::Gt))),
            TokenKind::GtEq => Some((3, Some(BinOp::Ge))),
            TokenKind::DotDot | TokenKind::DotDotEq => Some((4, None)),
            TokenKind::Pipe => Some((5, Some(BinOp::BitOr))),
            TokenKind::Caret => Some((6, Some(BinOp::BitXor))),
            TokenKind::Amp => Some((7, Some(BinOp::BitAnd))),
            TokenKind::Shl => Some((8, Some(BinOp::Shl))),
            TokenKind::Shr => Some((8, Some(BinOp::Shr))),
            TokenKind::Plus => Some((9, Some(BinOp::Add))),
            TokenKind::Minus => Some((9, Some(BinOp::Sub))),
            TokenKind::Star => Some((10, Some(BinOp::Mul))),
            TokenKind::Slash => Some((10, Some(BinOp::Div))),
            TokenKind::Percent => Some((10, Some(BinOp::Rem))),
            TokenKind::As => Some((11, None)),
            _ => None,
        }
    }

    fn parse_prec_expr(&mut self, min_prec: u8) -> Expr {
        let mut left = self.parse_unary_expr();

        loop {
            let Some((prec, maybe_op)) = Self::binop_prec(self.peek_kind()) else {
                break;
            };
            if prec < min_prec {
                break;
            }

            let tok_kind = self.advance().kind;
            if tok_kind == TokenKind::As {
                let ty = self.parse_type();
                let span = left.span.clone();
                left = Expr {
                    kind: ExprKind::Cast {
                        expr: Box::new(left),
                        ty,
                    },
                    span,
                };
                continue;
            }

            if tok_kind == TokenKind::DotDot || tok_kind == TokenKind::DotDotEq {
                let inclusive = tok_kind == TokenKind::DotDotEq;
                let right = self.parse_prec_expr(prec + 1);
                let span = left.span.clone();
                left = Expr {
                    kind: ExprKind::Range {
                        start: Box::new(left),
                        end: Box::new(right),
                        inclusive,
                    },
                    span,
                };
                continue;
            }

            let op = maybe_op.unwrap();
            let right = self.parse_prec_expr(prec + 1);
            let span = left.span.clone();
            left = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }

        left
    }

    fn parse_unary_expr(&mut self) -> Expr {
        let span = self.span();
        let unary_op = match self.peek_kind() {
            TokenKind::Minus => Some(UnaryOp::Neg),
            TokenKind::Bang => Some(UnaryOp::Not),
            TokenKind::Tilde => Some(UnaryOp::BitNot),
            TokenKind::Read => Some(UnaryOp::Read),
            TokenKind::Write => Some(UnaryOp::Write),
            TokenKind::Take => Some(UnaryOp::Take),
            TokenKind::Share => Some(UnaryOp::Share),
            TokenKind::Await => Some(UnaryOp::Await),
            TokenKind::Spawn => Some(UnaryOp::Spawn),
            _ => None,
        };

        if let Some(op) = unary_op {
            self.advance();
            let expr = self.parse_unary_expr();
            Expr {
                kind: ExprKind::Unary {
                    op,
                    expr: Box::new(expr),
                },
                span,
            }
        } else {
            self.parse_postfix_expr()
        }
    }

    fn parse_call_args(&mut self) -> Vec<CallArg> {
        let mut args = Vec::new();
        while !self.is_eof() && !self.eat(&TokenKind::RParen) {
            let cap = self.parse_capability();
            let expr = self.parse_expr();
            args.push(CallArg {
                capability: cap,
                expr,
            });
            if !self.eat(&TokenKind::Comma) {
                let _ = self.eat(&TokenKind::RParen);
                break;
            }
        }
        args
    }

    fn parse_postfix_expr(&mut self) -> Expr {
        let mut expr = self.parse_primary_expr();

        loop {
            // Do not chain across a newline if the next token is `(` or `[`
            if self.eat(&TokenKind::Dot) {
                let (mut member, _) = self.expect_ident("field or method name");
                // Optional turbofish `.<T>(...)`
                if self.peek_kind() == &TokenKind::ColonColon {
                    self.advance();
                    let m_gen = self.parse_generics();
                    if !m_gen.is_empty() {
                        let arg_str = m_gen
                            .iter()
                            .map(|g| g.name.as_str())
                            .collect::<Vec<_>>()
                            .join("$");
                        member = format!("{}${}", member, arg_str);
                    }
                }
                if self.eat(&TokenKind::LParen) {
                    let args = self.parse_call_args();
                    let span = expr.span.clone();
                    expr = Expr {
                        kind: ExprKind::MethodCall {
                            receiver: Box::new(expr),
                            method: member,
                            args,
                        },
                        span,
                    };
                } else {
                    let span = expr.span.clone();
                    expr = Expr {
                        kind: ExprKind::FieldAccess {
                            base: Box::new(expr),
                            field: member,
                        },
                        span,
                    };
                }
            } else if !self.peek().preceded_by_newline && self.eat(&TokenKind::LParen) {
                let args = self.parse_call_args();
                let span = expr.span.clone();
                expr = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    span,
                };
            } else if !self.peek().preceded_by_newline && self.eat(&TokenKind::LBracket) {
                let index = self.parse_expr();
                self.expect(&TokenKind::RBracket, "`]`");
                let span = expr.span.clone();
                expr = Expr {
                    kind: ExprKind::Index {
                        base: Box::new(expr),
                        index: Box::new(index),
                    },
                    span,
                };
            } else if !self.peek().preceded_by_newline && self.eat(&TokenKind::Question) {
                let span = expr.span.clone();
                expr = Expr {
                    kind: ExprKind::TryProp(Box::new(expr)),
                    span,
                };
            } else {
                break;
            }
        }

        expr
    }

    fn is_struct_init_ahead(&self) -> bool {
        if !self.allow_struct_lit || self.peek_kind() != &TokenKind::LBrace {
            return false;
        }
        // Look ahead inside `{`: either `{}` or `{ ident : ... }`
        match (
            self.tokens.get(self.pos + 1).map(|t| &t.kind),
            self.tokens.get(self.pos + 2).map(|t| &t.kind),
        ) {
            (Some(TokenKind::RBrace), _) => true,
            (Some(TokenKind::Ident(_)), Some(TokenKind::Colon)) => true,
            _ => false,
        }
    }

    fn parse_primary_expr(&mut self) -> Expr {
        let span = self.span();
        match self.peek_kind().clone() {
            TokenKind::IntLit(val, suffix) => {
                self.advance();
                Expr {
                    kind: ExprKind::IntLit(val, suffix),
                    span,
                }
            }
            TokenKind::FloatLit(val, suffix) => {
                self.advance();
                Expr {
                    kind: ExprKind::FloatLit(val, suffix),
                    span,
                }
            }
            TokenKind::StringLit(s) => {
                self.advance();
                Expr {
                    kind: ExprKind::StringLit(s),
                    span,
                }
            }
            TokenKind::CharLit(c) => {
                self.advance();
                Expr {
                    kind: ExprKind::CharLit(c),
                    span,
                }
            }
            TokenKind::BoolLit(b) => {
                self.advance();
                Expr {
                    kind: ExprKind::BoolLit(b),
                    span,
                }
            }
            TokenKind::If => self.parse_if_expr(),
            TokenKind::Match => self.parse_match_expr(),
            TokenKind::LBrace => Expr {
                kind: ExprKind::Block(self.parse_block()),
                span,
            },
            TokenKind::LBracket => {
                self.advance();
                let mut elems = Vec::new();
                while !self.is_eof() && !self.eat(&TokenKind::RBracket) {
                    let elem = self.parse_expr();
                    if self.eat(&TokenKind::Semi) {
                        // Repeat syntax `[val; count]`
                        if let TokenKind::IntLit(count, _) = self.peek_kind().clone() {
                            self.advance();
                            for _ in 0..(count.max(1) as usize) {
                                elems.push(elem.clone());
                            }
                        } else {
                            let _ = self.parse_expr();
                            elems.push(elem);
                        }
                        self.expect(&TokenKind::RBracket, "`]`");
                        break;
                    }
                    elems.push(elem);
                    if !self.eat(&TokenKind::Comma) {
                        let _ = self.eat(&TokenKind::RBracket);
                        break;
                    }
                }
                Expr {
                    kind: ExprKind::ArrayLit(elems),
                    span,
                }
            }
            TokenKind::LParen => {
                self.advance();
                if self.eat(&TokenKind::RParen) {
                    return Expr {
                        kind: ExprKind::IntLit(0, None),
                        span,
                    };
                }
                let prev = self.allow_struct_lit;
                self.allow_struct_lit = true;
                let inner = self.parse_expr();
                self.allow_struct_lit = prev;
                self.expect(&TokenKind::RParen, "`)`");
                inner
            }
            TokenKind::Pipe | TokenKind::OrOr => self.parse_closure_expr(),
            TokenKind::Ident(mut name) => {
                self.advance();
                // Check `::` path or generic instantiation `Type::<T>`
                while self.eat(&TokenKind::ColonColon) {
                    if self.peek_kind() == &TokenKind::Lt {
                        let gen_args = self.parse_generics();
                        let is_builtin = matches!(
                            name.as_str(),
                            "Vec"
                                | "HashMap"
                                | "HashSet"
                                | "Option"
                                | "Result"
                                | "Channel"
                                | "channel"
                                | "Receiver"
                                | "Sender"
                                | "Task"
                                | "Shared"
                                | "Weak"
                                | "Mutex"
                                | "spawn_thread"
                                | "block_on"
                                | "promote"
                        );
                        if !is_builtin && !gen_args.is_empty() {
                            let arg_str = gen_args
                                .iter()
                                .map(|g| g.name.as_str())
                                .collect::<Vec<_>>()
                                .join("$");
                            name = format!("{}${}", name, arg_str);
                        }
                        break;
                    }
                    let (seg, _) = self.expect_ident("path segment");
                    name = format!("{}::{}", name, seg);
                }

                // Check generic struct init `Pair<i64> { ... }` or generic type call `min_val<f64>(...)`
                if self.peek_kind() == &TokenKind::Lt {
                    let saved_pos = self.pos;
                    let saved_diag_len = self.diagnostics.diagnostics.len();
                    let gen_args = self.parse_generics();
                    if !gen_args.is_empty()
                        && (self.is_struct_init_ahead()
                            || self.peek_kind() == &TokenKind::LParen
                            || self.peek_kind() == &TokenKind::ColonColon)
                    {
                        // Valid generic type or function instantiation
                        let is_builtin = matches!(
                            name.as_str(),
                            "Vec"
                                | "HashMap"
                                | "HashSet"
                                | "Option"
                                | "Result"
                                | "Channel"
                                | "channel"
                                | "Receiver"
                                | "Sender"
                                | "Task"
                                | "Shared"
                                | "Weak"
                                | "Mutex"
                                | "spawn_thread"
                                | "block_on"
                                | "promote"
                        );
                        if !is_builtin && !gen_args.is_empty() {
                            let arg_str = gen_args
                                .iter()
                                .map(|g| g.name.as_str())
                                .collect::<Vec<_>>()
                                .join("$");
                            name = format!("{}${}", name, arg_str);
                        }
                    } else {
                        self.pos = saved_pos;
                        self.diagnostics.diagnostics.truncate(saved_diag_len);
                    }
                }

                if self.is_struct_init_ahead() {
                    self.expect(&TokenKind::LBrace, "`{`");
                    let mut fields = Vec::new();
                    while !self.is_eof() && !self.eat(&TokenKind::RBrace) {
                        let (fname, _) = self.expect_ident("field name");
                        let fval = if self.eat(&TokenKind::Colon) {
                            self.parse_expr()
                        } else {
                            Expr {
                                kind: ExprKind::Ident(fname.clone()),
                                span: self.span(),
                            }
                        };
                        fields.push((fname, fval));
                        let _ = self.eat(&TokenKind::Comma) || self.eat(&TokenKind::Semi);
                    }
                    Expr {
                        kind: ExprKind::StructInit { name, fields },
                        span,
                    }
                } else {
                    Expr {
                        kind: ExprKind::Ident(name),
                        span,
                    }
                }
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    "E0013",
                    format!("expected expression, found `{:?}`", self.peek_kind()),
                    span.clone(),
                ));
                self.advance();
                Expr {
                    kind: ExprKind::IntLit(0, None),
                    span,
                }
            }
        }
    }

    fn parse_if_expr(&mut self) -> Expr {
        let span = self.expect(&TokenKind::If, "`if`");
        let prev = self.allow_struct_lit;
        self.allow_struct_lit = false;
        let cond = self.parse_expr();
        self.allow_struct_lit = prev;
        let then_block = self.parse_block();
        let else_branch = if self.eat(&TokenKind::Else) {
            if self.peek_kind() == &TokenKind::If {
                Some(Box::new(self.parse_if_expr()))
            } else {
                let blk = self.parse_block();
                let bs = blk.span.clone();
                Some(Box::new(Expr {
                    kind: ExprKind::Block(blk),
                    span: bs,
                }))
            }
        } else {
            None
        };
        Expr {
            kind: ExprKind::If {
                cond: Box::new(cond),
                then_block,
                else_branch,
            },
            span,
        }
    }

    fn parse_match_expr(&mut self) -> Expr {
        let span = self.expect(&TokenKind::Match, "`match`");
        let prev = self.allow_struct_lit;
        self.allow_struct_lit = false;
        let scrutinee = self.parse_expr();
        self.allow_struct_lit = prev;
        self.expect(&TokenKind::LBrace, "`{`");
        let mut arms = Vec::new();
        while !self.is_eof() && !self.eat(&TokenKind::RBrace) {
            let arm_span = self.span();
            let pattern = self.parse_pattern();
            self.expect(&TokenKind::FatArrow, "`=>`");
            let body = if self.peek_kind() == &TokenKind::LBrace {
                self.parse_block()
            } else {
                let expr = self.parse_expr();
                let es = expr.span.clone();
                Block {
                    stmts: vec![Stmt::Expr {
                        expr,
                        has_semi: false,
                        span: es.clone(),
                    }],
                    span: es,
                }
            };
            let _ = self.eat(&TokenKind::Comma) || self.eat(&TokenKind::Semi);
            arms.push(MatchArm {
                pattern,
                body,
                span: arm_span,
            });
        }
        Expr {
            kind: ExprKind::Match {
                scrutinee: Box::new(scrutinee),
                arms,
            },
            span,
        }
    }

    fn parse_pattern(&mut self) -> Pattern {
        match self.peek_kind().clone() {
            TokenKind::Ident(s) if s == "_" => {
                self.advance();
                Pattern::Wildcard
            }
            TokenKind::IntLit(_, _)
            | TokenKind::FloatLit(_, _)
            | TokenKind::StringLit(_)
            | TokenKind::BoolLit(_)
            | TokenKind::CharLit(_)
            | TokenKind::Minus => {
                let prev = self.allow_struct_lit;
                self.allow_struct_lit = false;
                let lit = self.parse_unary_expr();
                if self.eat(&TokenKind::DotDot) || self.eat(&TokenKind::DotDotEq) {
                    let _ = self.parse_unary_expr();
                }
                while self.eat(&TokenKind::Pipe) {
                    let _ = self.parse_unary_expr();
                }
                self.allow_struct_lit = prev;
                Pattern::Literal(lit)
            }
            TokenKind::Ident(first) => {
                self.advance();
                let (enum_name, variant_name) =
                    if self.eat(&TokenKind::Dot) || self.eat(&TokenKind::ColonColon) {
                        let (var, _) = self.expect_ident("variant name");
                        (Some(first), var)
                    } else {
                        (None, first)
                    };

                if self.eat(&TokenKind::LParen) {
                    let mut bindings = Vec::new();
                    while !self.is_eof() && !self.eat(&TokenKind::RParen) {
                        let (b, _) = self.expect_ident("pattern binding");
                        bindings.push(b);
                        if !self.eat(&TokenKind::Comma) {
                            let _ = self.eat(&TokenKind::RParen);
                            break;
                        }
                    }
                    Pattern::Variant {
                        enum_name,
                        variant_name,
                        bindings,
                    }
                } else if self.eat(&TokenKind::LBrace) {
                    let mut bindings = Vec::new();
                    while !self.is_eof() && !self.eat(&TokenKind::RBrace) {
                        let (b, _) = self.expect_ident("pattern field");
                        if self.eat(&TokenKind::Colon) {
                            let (alias, _) = self.expect_ident("pattern binding");
                            bindings.push(alias);
                        } else {
                            bindings.push(b);
                        }
                        let _ = self.eat(&TokenKind::Comma) || self.eat(&TokenKind::Semi);
                    }
                    Pattern::Variant {
                        enum_name,
                        variant_name,
                        bindings,
                    }
                } else if enum_name.is_some()
                    || variant_name == "None"
                    || variant_name
                        .chars()
                        .next()
                        .map(|c| c.is_uppercase())
                        .unwrap_or(false)
                {
                    Pattern::Variant {
                        enum_name,
                        variant_name,
                        bindings: Vec::new(),
                    }
                } else {
                    Pattern::Ident(variant_name)
                }
            }
            _ => {
                self.advance();
                Pattern::Wildcard
            }
        }
    }

    fn parse_closure_expr(&mut self) -> Expr {
        let span = self.span();
        let mut params = Vec::new();
        if self.eat(&TokenKind::OrOr) {
            // `||` empty params
        } else {
            self.expect(&TokenKind::Pipe, "`|`");
            while !self.is_eof() && !self.eat(&TokenKind::Pipe) {
                let p_span = self.span();
                let cap = self.parse_capability();
                let (name, _) = self.expect_ident("closure parameter");
                let ty = if self.eat(&TokenKind::Colon) {
                    self.parse_type()
                } else {
                    TypeAst::Inferred
                };
                params.push(Param {
                    name,
                    capability: cap,
                    ty,
                    is_mutable: cap == Capability::Write,
                    span: p_span,
                });
                if !self.eat(&TokenKind::Comma) {
                    let _ = self.eat(&TokenKind::Pipe);
                    break;
                }
            }
        }
        let ret_type = if self.eat(&TokenKind::Arrow) {
            self.parse_type()
        } else {
            TypeAst::Inferred
        };
        let body = if self.peek_kind() == &TokenKind::LBrace {
            self.parse_block()
        } else {
            let expr = self.parse_expr();
            let es = expr.span.clone();
            Block {
                stmts: vec![Stmt::Expr {
                    expr,
                    has_semi: false,
                    span: es.clone(),
                }],
                span: es,
            }
        };
        Expr {
            kind: ExprKind::Closure {
                params,
                ret_type,
                body,
            },
            span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::lexer::Lexer;

    #[test]
    fn test_parse_driver_and_functions() {
        let src = r#"
struct Driver {
    id: i64
    name: String
    balance: i64
}

fn add_balance(driver: write Driver, amount: i64) {
    driver.balance += amount
}

fn main() {
    var driver = Driver {
        id: 1,
        name: "Alibek",
        balance: 1000
    }
    add_balance(write driver, 500)
    print(driver.name)
}
"#;
        let mut lexer = Lexer::new("test.aps", src);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        let prog = parser.parse_program();
        assert!(!parser.diagnostics.has_errors());
        assert_eq!(prog.items.len(), 3);
    }

    #[test]
    fn test_parse_less_than_binary_op_not_confused_with_generics() {
        let src = r#"
struct Config {
    min_val: i64
}

fn check_threshold(net: i64, cfg: Config) -> bool {
    if net < cfg.min_val {
        return false
    }
    return true
}
"#;
        let mut lexer = Lexer::new("test.aps", src);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        let prog = parser.parse_program();
        assert!(!parser.diagnostics.has_errors());
        assert_eq!(prog.items.len(), 2);
    }
}
