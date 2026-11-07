pub mod ama;
pub mod ast;
pub mod codegen;
pub mod diagnostics;
pub mod hir;
pub mod ir;
pub mod lexer;
pub mod mir;
pub mod optimize;
pub mod parser;
pub mod semantic;
pub mod types;

use crate::compiler::ama::{AmaAnalyzer, AmaReport};
use crate::compiler::ast::Program;
use crate::compiler::codegen::LlvmBackend;
use crate::compiler::diagnostics::DiagnosticBag;
use crate::compiler::hir::HirLowering;
use crate::compiler::ir::ApsIrModule;
use crate::compiler::lexer::Lexer;
use crate::compiler::mir::MirBuilder;
use crate::compiler::optimize::{OptStats, Optimizer};
use crate::compiler::parser::Parser;
use crate::compiler::semantic::SemanticAnalyzer;
use std::path::Path;

#[derive(Debug)]
pub struct CompilationResult {
    pub program: Program,
    pub ama_report: AmaReport,
    pub opt_stats: OptStats,
    pub aps_ir_text: String,
    pub llvm_ir: String,
    pub diagnostics: DiagnosticBag,
}

pub struct CompilerPipeline {
    pub opt_level: u8,
    pub target_triple: Option<String>,
}

impl CompilerPipeline {
    pub fn new(opt_level: u8) -> Self {
        Self {
            opt_level,
            target_triple: None,
        }
    }

    pub fn check_source(&self, filename: &str, source: &str) -> CompilationResult {
        let mut all_diags = DiagnosticBag::new();

        // 1. Lexer
        let mut lexer = Lexer::new(filename, source);
        let tokens = lexer.tokenize();
        for d in lexer.diagnostics.diagnostics {
            all_diags.push(d);
        }

        // 2. Parser -> AST
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program();
        for d in parser.diagnostics.diagnostics {
            all_diags.push(d);
        }

        // 3. Semantic Analysis & Type Checking
        let mut sem = SemanticAnalyzer::new();
        sem.analyze_program(&program);
        for d in sem.diagnostics.diagnostics {
            all_diags.push(d);
        }

        // 4. AMA Memory Safety & Placement Analysis
        let mut ama = AmaAnalyzer::new(sem.tcx.clone(), sem.functions.clone());
        ama.analyze_program(&program);
        for d in ama.diagnostics.diagnostics {
            all_diags.push(d);
        }
        let ama_report = ama.report;

        // 5. HIR Lowering
        let hir = HirLowering::new(sem.tcx).lower_program(&program);

        // 6. MIR Construction
        let mir = MirBuilder::build(hir, ama_report.clone());

        // 7. APS IR & Optimizer
        let mut aps_ir = ApsIrModule::from_mir(mir);
        let mut optimizer = Optimizer::new(self.opt_level);
        optimizer.optimize_module(&mut aps_ir);
        let aps_ir_text = aps_ir.dump_text();
        let opt_stats = optimizer.stats;

        // 8. LLVM IR Generation
        let mut backend = LlvmBackend::new(self.target_triple.as_deref());
        let llvm_ir = if !all_diags.has_errors() {
            backend.generate_llvm_ir(&aps_ir)
        } else {
            String::new()
        };

        CompilationResult {
            program,
            ama_report,
            opt_stats,
            aps_ir_text,
            llvm_ir,
            diagnostics: all_diags,
        }
    }

    pub fn build_executable(
        &self,
        filename: &str,
        source: &str,
        output_exe: &Path,
    ) -> Result<CompilationResult, String> {
        let res = self.check_source(filename, source);
        if res.diagnostics.has_errors() {
            return Err(res.diagnostics.render_all(source));
        }
        let backend = LlvmBackend::new(self.target_triple.as_deref());
        backend.compile_to_executable(&res.llvm_ir, output_exe, self.opt_level)?;
        Ok(res)
    }
}
