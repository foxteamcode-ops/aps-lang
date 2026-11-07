use crate::compiler::ast::*;
use crate::compiler::diagnostics::{Diagnostic, DiagnosticBag};

pub fn lint_program(program: &Program) -> DiagnosticBag {
    let mut bag = DiagnosticBag::new();

    for item in &program.items {
        match item {
            Item::Struct(s) => {
                if !s
                    .name
                    .chars()
                    .next()
                    .map(|c| c.is_uppercase())
                    .unwrap_or(false)
                {
                    bag.push(
                        Diagnostic::warning(
                            "W0001",
                            format!("struct `{}` should have a PascalCase name", s.name),
                            s.span.clone(),
                        )
                        .with_help("capitalize the first letter of struct names"),
                    );
                }
            }
            Item::Function(f) => {
                if f.name.chars().any(|c| c.is_uppercase()) {
                    bag.push(
                        Diagnostic::warning(
                            "W0002",
                            format!("function `{}` should have a snake_case name", f.name),
                            f.span.clone(),
                        )
                        .with_help("use lowercase letters and underscores for function names"),
                    );
                }
            }
            Item::Const(c) => {
                if c.name.chars().any(|ch| ch.is_lowercase()) {
                    bag.push(
                        Diagnostic::warning(
                            "W0003",
                            format!(
                                "constant `{}` should have a SCREAMING_SNAKE_CASE name",
                                c.name
                            ),
                            c.span.clone(),
                        )
                        .with_help("use uppercase letters and underscores for constants"),
                    );
                }
            }
            _ => {}
        }
    }

    bag
}
