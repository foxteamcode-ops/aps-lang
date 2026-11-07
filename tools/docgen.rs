use crate::compiler::ast::*;

pub fn generate_markdown_docs(module_title: &str, program: &Program) -> String {
    let mut doc = String::new();
    doc.push_str(&format!("# APS API Documentation — `{}`\n\n", module_title));

    if let Some(ref mpath) = program.module_name {
        doc.push_str(&format!("**Module:** `{}`\n\n", mpath.join(".")));
    }

    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut functions = Vec::new();
    let mut consts = Vec::new();

    for item in &program.items {
        match item {
            Item::Struct(s) => structs.push(s),
            Item::Enum(e) => enums.push(e),
            Item::Function(f) => functions.push(f),
            Item::Const(c) => consts.push(c),
            _ => {}
        }
    }

    if !structs.is_empty() {
        doc.push_str("## Structs\n\n");
        for s in structs {
            doc.push_str(&format!("### `struct {}`\n\n", s.name));
            doc.push_str("| Field | Type |\n|---|---|\n");
            for f in &s.fields {
                doc.push_str(&format!("| `{}` | `{:?}` |\n", f.name, f.ty));
            }
            doc.push('\n');
        }
    }

    if !enums.is_empty() {
        doc.push_str("## Enums\n\n");
        for e in enums {
            doc.push_str(&format!("### `enum {}`\n\n", e.name));
            for v in &e.variants {
                doc.push_str(&format!("- `{}`\n", v.name));
            }
            doc.push('\n');
        }
    }

    if !functions.is_empty() {
        doc.push_str("## Functions\n\n");
        for f in functions {
            let ps: Vec<String> = f
                .params
                .iter()
                .map(|p| {
                    if p.capability == Capability::Value {
                        format!("{}: {:?}", p.name, p.ty)
                    } else {
                        format!("{}: {} {:?}", p.name, p.capability.as_str(), p.ty)
                    }
                })
                .collect();
            doc.push_str(&format!(
                "- `fn {}({}) -> {:?}` (line {})\n",
                f.name,
                ps.join(", "),
                f.ret_type,
                f.span.line
            ));
        }
        doc.push('\n');
    }

    if !consts.is_empty() {
        doc.push_str("## Constants\n\n");
        for c in consts {
            doc.push_str(&format!("- `const {}` (line {})\n", c.name, c.span.line));
        }
        doc.push('\n');
    }

    doc
}
