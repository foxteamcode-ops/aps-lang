use crate::compiler::ama::AmaReport;
use crate::compiler::ast::*;
use crate::compiler::hir::{HirEnum, HirStruct};
use crate::compiler::mir::MirModule;
use crate::compiler::types::Type;

#[derive(Debug, Clone)]
pub struct ApsIrModule {
    pub structs: Vec<HirStruct>,
    pub enums: Vec<HirEnum>,
    pub consts: Vec<(String, Type, Expr)>,
    pub externs: Vec<FnSig>,
    pub functions: Vec<ApsIrFunction>,
    pub ama_report: AmaReport,
}

#[derive(Debug, Clone)]
pub struct ApsIrFunction {
    pub name: String,
    pub params: Vec<(String, Capability, Type, bool)>,
    pub ret_type: Type,
    pub is_async: bool,
    pub inline_hint: bool,
    pub body: Block,
    pub scheduled_drops: Vec<String>,
}

impl ApsIrModule {
    pub fn from_mir(mir: MirModule) -> Self {
        let functions = mir
            .functions
            .into_iter()
            .map(|f| {
                let inline_hint = f.name != "main" && f.body.stmts.len() <= 4;
                ApsIrFunction {
                    name: f.name,
                    params: f.params,
                    ret_type: f.ret_type,
                    is_async: f.is_async,
                    inline_hint,
                    body: f.body,
                    scheduled_drops: f.scheduled_drops,
                }
            })
            .collect();

        Self {
            structs: mir.structs,
            enums: mir.enums,
            consts: mir.consts,
            externs: mir.externs,
            functions,
            ama_report: mir.ama_report,
        }
    }

    pub fn dump_text(&self) -> String {
        let mut s = String::new();
        s.push_str("; === APS Intermediate Representation (APS IR) ===\n");
        for st in &self.structs {
            let fs: Vec<String> = st
                .fields
                .iter()
                .map(|(n, t)| format!("{}: {}", n, t))
                .collect();
            s.push_str(&format!("struct {} {{ {} }}\n", st.name, fs.join(", ")));
        }
        for en in &self.enums {
            let vs: Vec<String> = en.variants.iter().map(|(n, _)| n.clone()).collect();
            s.push_str(&format!("enum {} {{ {} }}\n", en.name, vs.join(" | ")));
        }
        for f in &self.functions {
            let ps: Vec<String> = f
                .params
                .iter()
                .map(|(n, c, t, _)| {
                    if *c == Capability::Value {
                        format!("{}: {}", n, t)
                    } else {
                        format!("{}: {} {}", n, c.as_str(), t)
                    }
                })
                .collect();
            s.push_str(&format!(
                "fn {}({}) -> {} [stmts={}, inline={}, drops={:?}]\n",
                f.name,
                ps.join(", "),
                f.ret_type,
                f.body.stmts.len(),
                f.inline_hint,
                f.scheduled_drops
            ));
        }
        s
    }
}
