use liquid_rust_common::{SemiGroup, errors::ErrorReported, iter::IterExt};
use liquid_rust_core::{wf::Wf, ir::Body};
use liquid_rust_typeck::{
    global_env::{FnSpec, GlobalEnv},
    Checker,
};
use liquid_rust_fixpoint::{ FixpointResult };
use rustc_driver::{Callbacks, Compilation};
use rustc_hash::FxHashMap;
use rustc_hir::def_id::DefId;
use rustc_interface::{interface::Compiler, Queries};
use rustc_middle::ty::TyCtxt;
use rustc_session::Session;

use crate::{collector::SpecCollector, lowering::LoweringCtxt, resolve::Resolver};

/// Compiler callbacks for Liquid Rust.
#[derive(Default)]
pub(crate) struct LiquidCallbacks {
    pub result : FixpointResult,
}

impl Callbacks for LiquidCallbacks {
    fn after_analysis<'tcx>(
        &mut self,
        compiler: &Compiler,
        queries: &'tcx Queries<'tcx>,
    ) -> Compilation {
        queries.global_ctxt().unwrap().peek_mut().enter(|tcx| {
            let res = check_crate(tcx, compiler.session());
            match res {
                Ok(r) => self.result = self.result.append(r),
                Err(_) => ()
            }
        });

        Compilation::Stop
    }
}

fn gather_body_def_ids(body: &Body, res: &mut Vec<DefId>) {
    for (_bb, bb_data) in body.basic_blocks.iter_enumerated() {
        if let Some(term) = &bb_data.terminator {
            match term.kind {
                liquid_rust_core::ir::TerminatorKind::Call{ func, .. } => res.push(func),
                _ => ()
            }
        }
    }
}

fn gather_def_ids(annotations: &std::collections::HashMap<rustc_hir::def_id::LocalDefId, crate::collector::FnSpec, std::hash::BuildHasherDefault<rustc_hash::FxHasher>>, tcx: TyCtxt) -> Result<Vec<DefId>, ErrorReported> {
    let mut def_ids: Vec<DefId> = vec!();
    for def_id in annotations.keys() {
        let body = LoweringCtxt::lower(tcx, tcx.optimized_mir(*def_id))?;
        gather_body_def_ids(&body, &mut def_ids)
    }
    Ok(def_ids)
}

fn check_crate(tcx: TyCtxt, sess: &Session) -> Result<FixpointResult, ErrorReported> {
    let annotations = SpecCollector::collect(tcx, sess)?;

    // walk over all defs to gather all called DefId
    let def_ids = gather_def_ids(&annotations, tcx)?;
    println!("Used DefIds: {:?}", def_ids);
    
    let wf = Wf::new(sess);
    let fn_sigs: FxHashMap<_, _> = annotations
        .into_iter()
        .map(|(def_id, spec)| {
            let fn_sig = Resolver::resolve(tcx, def_id, spec.fn_sig)?;
            wf.check_fn_sig(&fn_sig)?;
            Ok((
                def_id,
                FnSpec {
                    fn_sig,
                    assume: spec.assume,
                },
            ))
        })
        .try_collect_exhaust()?;


    let global_env = GlobalEnv::new(tcx, fn_sigs);
    global_env
        .specs
        .iter()
        .map(|(def_id, spec)| {
            if spec.assume {
                return Ok(Default::default());
            }
            println!("\n-------------------------------------------------");
            println!("CHECKING: {}", tcx.item_name(def_id.to_def_id()));
            println!("-------------------------------------------------");
            let body = LoweringCtxt::lower(tcx, tcx.optimized_mir(*def_id))?;
            Checker::check(&global_env, &body, &spec.fn_sig)
        })
        .try_collect_exhaust()
}

