use crate::compiler::ama::AmaReport;
use crate::compiler::ast::*;
use crate::compiler::hir::{HirEnum, HirModule, HirStruct};
use crate::compiler::types::Type;

#[derive(Debug, Clone)]
pub struct MirModule {
    pub structs: Vec<HirStruct>,
    pub enums: Vec<HirEnum>,
    pub consts: Vec<(String, Type, Expr)>,
    pub externs: Vec<FnSig>,
    pub functions: Vec<MirFunction>,
    pub ama_report: AmaReport,
}

#[derive(Debug, Clone)]
pub struct MirFunction {
    pub name: String,
    pub params: Vec<(String, Capability, Type, bool)>,
    pub ret_type: Type,
    pub is_async: bool,
    pub body: Block,
    pub scheduled_drops: Vec<String>,
}

pub struct MirBuilder;

impl MirBuilder {
    pub fn build(hir: HirModule, ama_report: AmaReport) -> MirModule {
        let mut functions = Vec::new();
        for f in hir.functions {
            let scheduled_drops: Vec<String> = ama_report
                .ctfp_drops
                .iter()
                .map(|(v, _)| v.clone())
                .collect();
            functions.push(MirFunction {
                name: f.name,
                params: f.params,
                ret_type: f.ret_type,
                is_async: f.is_async,
                body: f.body,
                scheduled_drops,
            });
        }
        MirModule {
            structs: hir.structs,
            enums: hir.enums,
            consts: hir.consts,
            externs: hir.externs,
            functions,
            ama_report,
        }
    }
}
