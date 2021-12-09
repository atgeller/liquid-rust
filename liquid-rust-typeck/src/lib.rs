#![feature(rustc_private, min_specialization, once_cell)]
#![allow(warnings)]

extern crate rustc_data_structures;
extern crate rustc_hash;
extern crate rustc_hir;
extern crate rustc_index;
extern crate rustc_middle;
extern crate rustc_serialize;
extern crate rustc_session;
extern crate rustc_span;

mod constraint_builder;
pub mod global_env;
mod inference;
mod intern;
mod lowering;
mod pretty;
pub mod ty;
mod tyenv;

use crate::{
    constraint_builder::{ConstraintBuilder, Cursor},
    inference::InferCtxt,
    ty::{BaseTy, BinOp, Expr, ExprKind, KVid, RVid, Region, Ty, TyKind, Var},
};
use global_env::GlobalEnv;
use itertools::Itertools;
use liquid_rust_common::{
    disjoint_sets::DisjointSetsMap,
    errors::ErrorReported,
    index::{Idx, IndexGen},
};
use liquid_rust_core::{
    ir::{
        self, BasicBlock, Body, Constant, Local, Operand, Place, Rvalue, SourceInfo, Statement,
        StatementKind, Terminator, TerminatorKind, RETURN_PLACE, START_BLOCK,
    },
    ty::{self as core, Name},
};
use liquid_rust_fixpoint::Fixpoint;
use rustc_data_structures::graph::dominators::Dominators;
use rustc_hash::FxHashMap;
use rustc_hir::def_id::DefId;
use rustc_index::bit_set::BitSet;
use rustc_middle::mir;
use rustc_session::Session;
use rustc_span::MultiSpan;
use tyenv::{TyEnv, TyEnvBuilder};

pub struct Checker<'a, 'tcx> {
    sess: &'a Session,
    body: &'a Body<'tcx>,
    // Whether the block immediately domminates a join point.
    dominates_join_point: BitSet<BasicBlock>,
    visited: BitSet<BasicBlock>,
    bb_envs: FxHashMap<BasicBlock, TyEnv>,
    bb_env_shapes: FxHashMap<BasicBlock, DisjointSetsMap<RVid, inference::Ty>>,
    ret_ty: Ty,
    global_env: &'a GlobalEnv<'tcx>,
    ensures: Vec<(Region, Ty)>,
}

impl Checker<'_, '_> {
    pub fn check<'tcx>(
        global_env: &GlobalEnv<'tcx>,
        body: &Body<'tcx>,
        fn_sig: &core::FnSig,
    ) -> Result<liquid_rust_fixpoint::FixpointResult, ErrorReported> {
        let bb_env_shapes = InferCtxt::infer(global_env, body, fn_sig);

        let mut constraint = ConstraintBuilder::new(global_env.tcx);
        let cursor = &mut constraint.as_cursor();

        let mut env = TyEnvBuilder::new(body.nlocals);
        let mut subst = lowering::Subst::with_empty_type_substs();

        for param in &fn_sig.params {
            let fresh = cursor.fresh_name();
            subst.insert_expr(param.name.name, Var::Free(fresh));
            cursor.push_forall(fresh, param.sort, subst.lower_expr(&param.pred));
        }

        for (name, ty) in &fn_sig.requires {
            let ty = subst.lower_ty(cursor, ty);
            let rvid = env.define_abstract_region(cursor.unpack(ty));
            subst.insert_region(*name, rvid);
        }

        for (local, ty) in body.args_iter().zip(&fn_sig.args) {
            let ty = subst.lower_ty(cursor, ty);
            env.define_local(local, cursor.unpack(ty));
        }

        for local in body.vars_and_temps_iter() {
            env.define_local(local, TyKind::Uninit.intern())
        }

        env.define_local(RETURN_PLACE, ty::TyKind::Uninit.intern());

        let ensures = fn_sig
            .ensures
            .iter()
            .map(|(name, ty)| {
                let ty = subst.lower_ty(cursor, ty);
                let region = subst.lower_region(*name).unwrap();
                (region, ty)
            })
            .collect();

        let ret_ty = subst.lower_ty(cursor, &fn_sig.ret);

        Checker::new(global_env, body, bb_env_shapes, ret_ty, ensures)
            .run(&mut env.build(), cursor)?;

        println!("{:?}", constraint);
        let constraint = constraint.to_fixpoint();
        let fixpoint_res = Fixpoint::check(&constraint);
        match fixpoint_res {
            Ok(r)  => Ok(r),
            Err(e) => Err(ErrorReported),
        }
    }

    fn new<'a, 'tcx>(
        global_env: &'a GlobalEnv<'tcx>,
        body: &'a Body<'tcx>,
        bb_env_shapes: FxHashMap<BasicBlock, DisjointSetsMap<RVid, inference::Ty>>,
        ret_ty: Ty,
        ensures: Vec<(Region, Ty)>,
    ) -> Checker<'a, 'tcx> {
        let dominators = body.dominators();
        let mut dominates_join_point = BitSet::new_empty(body.basic_blocks.len());
        for bb in body.join_points() {
            dominates_join_point.insert(dominators.immediate_dominator(bb));
        }

        Checker {
            sess: global_env.tcx.sess,
            global_env,
            body,
            dominates_join_point,
            bb_envs: FxHashMap::default(),
            visited: BitSet::new_empty(body.basic_blocks.len()),
            bb_env_shapes,
            ret_ty,
            ensures,
        }
    }

    fn run(&mut self, env: &mut TyEnv, cursor: &mut Cursor) -> Result<(), ErrorReported> {
        self.check_goto(env, cursor, START_BLOCK)?;
        for bb in self.body.reverse_postorder() {
            if !self.visited.contains(bb) {
                let mut env = self.bb_envs[&bb].clone();
                env.unpack(cursor);
                self.check_basic_block(&mut env, cursor, bb)?;
            }
        }
        Ok(())
    }

    fn check_basic_block(
        &mut self,
        env: &mut TyEnv,
        cursor: &mut Cursor,
        bb: BasicBlock,
    ) -> Result<(), ErrorReported> {
        self.visited.insert(bb);

        if self.dominates_join_point.contains(bb) {
            cursor.push_scope();
        }

        let data = &self.body.basic_blocks[bb];
        for stmt in &data.statements {
            self.check_statement(env, cursor, stmt);
        }
        if let Some(terminator) = &data.terminator {
            self.check_terminator(env, cursor, terminator)?;
        }
        Ok(())
    }

    fn check_statement(&self, env: &mut TyEnv, cursor: &mut Cursor, stmt: &Statement) {
        match &stmt.kind {
            StatementKind::Assign(p, rvalue) => {
                let ty = self.check_rvalue(env, cursor, rvalue);
                // OWNERSHIP SAFETY CHECK
                env.write_place(cursor, p, ty);
            }
            StatementKind::Nop => {}
        }
    }

    fn check_terminator(
        &mut self,
        env: &mut TyEnv,
        cursor: &mut Cursor,
        terminator: &Terminator,
    ) -> Result<(), ErrorReported> {
        match &terminator.kind {
            TerminatorKind::Return => {
                let ret_place_ty = env.lookup_local(RETURN_PLACE);
                cursor.subtyping(ret_place_ty, self.ret_ty.clone());

                for (region, ensured_ty) in &self.ensures {
                    let actual_ty = env.lookup_region(region[0]).unwrap();
                    cursor.subtyping(actual_ty, ensured_ty.clone());
                }
            }
            TerminatorKind::Goto { target } => {
                self.check_goto(env, cursor, *target)?;
            }
            TerminatorKind::SwitchInt { discr, targets } => {
                self.check_switch_int(env, cursor, discr, targets)?;
            }
            TerminatorKind::Call {
                func,
                substs,
                args,
                destination,
            } => {
                self.check_call(
                    env,
                    cursor,
                    terminator.source_info,
                    *func,
                    substs,
                    args,
                    destination,
                )?;
            }
            TerminatorKind::Drop { place, target } => {
                let _ = env.move_place(place);
                self.check_goto(env, cursor, *target);
            }
        }
        Ok(())
    }

    fn check_call(
        &mut self,
        env: &mut TyEnv,
        cursor: &mut Cursor,
        source_info: SourceInfo,
        func: DefId,
        substs: &[core::Ty],
        args: &[Operand],
        destination: &Option<(Place, BasicBlock)>,
    ) -> Result<(), ErrorReported> {
        let fn_sig = self.global_env.lookup_fn_sig(func);
        let actuals = args
            .iter()
            .map(|arg| self.check_operand(env, arg))
            .collect_vec();

        let mut subst = lowering::Subst::with_type_substs(cursor, substs);
        if let Err(errors) = subst.infer_from_fn_call(env, &actuals, fn_sig) {
            return self.report_inference_error(source_info);
        };

        for param in &fn_sig.params {
            cursor.push_head(subst.lower_expr(&param.pred));
        }

        for (actual, formal) in actuals.into_iter().zip(&fn_sig.args) {
            let formal = subst.lower_ty(cursor, formal);
            cursor.subtyping(actual, formal);
        }

        for (region, required_ty) in &fn_sig.requires {
            let actual_ty = env
                .lookup_region(subst.lower_region(*region).unwrap()[0])
                .unwrap();
            let required_ty = subst.lower_ty(cursor, required_ty);
            cursor.subtyping(actual_ty, required_ty);
        }

        for (region, updated_ty) in &fn_sig.ensures {
            let updated_ty = subst.lower_ty(cursor, updated_ty);
            let updated_ty = cursor.unpack(updated_ty);
            if let Some(region) = subst.lower_region(*region) {
                env.update_region(cursor, region[0], updated_ty);
            } else {
                let rvid = env.push_region(updated_ty);
                subst.insert_region(*region, rvid);
            }
        }

        if let Some((p, bb)) = destination {
            let ret = subst.lower_ty(cursor, &fn_sig.ret);
            let ret = cursor.unpack(ret);
            env.write_place(cursor, p, ret);

            self.check_goto(env, cursor, *bb)?;
        }
        Ok(())
    }

    fn check_switch_int(
        &mut self,
        env: &mut TyEnv,
        cursor: &mut Cursor,
        discr: &Operand,
        targets: &mir::SwitchTargets,
    ) -> Result<(), ErrorReported> {
        let discr_ty = self.check_operand(env, discr);
        let mk = |bits| match discr_ty.kind() {
            TyKind::Refine(BaseTy::Bool, expr) => {
                if bits != 0 {
                    expr.clone()
                } else {
                    expr.not()
                }
            }
            TyKind::Refine(bty @ (BaseTy::Int(_) | BaseTy::Uint(_)), expr) => {
                ExprKind::BinaryOp(BinOp::Eq, expr.clone(), Expr::from_bits(bty, bits)).intern()
            }
            _ => unreachable!("unexpected discr_ty {:?}", discr_ty),
        };

        for (bits, bb) in targets.iter() {
            let cursor = &mut cursor.snapshot();
            cursor.push_guard(mk(bits));
            self.check_goto(&mut env.clone(), cursor, bb)?;
        }
        let otherwise = targets
            .iter()
            .map(|(bits, _)| mk(bits).not())
            .reduce(|e1, e2| ExprKind::BinaryOp(BinOp::And, e1, e2).intern());

        let cursor = &mut cursor.snapshot();
        if let Some(otherwise) = otherwise {
            cursor.push_guard(otherwise);
        }

        self.check_goto(env, cursor, targets.otherwise())?;
        Ok(())
    }

    fn check_goto(
        &mut self,
        env: &mut TyEnv,
        cursor: &mut Cursor,
        target: BasicBlock,
    ) -> Result<(), ErrorReported> {
        if self.body.is_join_point(target) {
            let bb_env = self.bb_envs.entry(target).or_insert_with(|| {
                env.infer_bb_env(cursor, self.bb_env_shapes.remove(&target).unwrap())
            });
            for (mut region, ty1) in env.iter() {
                // FIXME: we should check the region in env is a subset of a region in bb_env
                let local = region.next().unwrap();

                // We allow weakening
                if let Some(ty2) = bb_env.lookup_region(local) {
                    cursor.subtyping(ty1.ty(), ty2);
                }
            }
            Ok(())
        } else {
            self.check_basic_block(env, cursor, target)
        }
    }

    fn check_rvalue(&self, env: &mut TyEnv, cursor: &mut Cursor, rvalue: &Rvalue) -> Ty {
        match rvalue {
            Rvalue::Use(operand) => self.check_operand(env, operand),
            Rvalue::BinaryOp(bin_op, op1, op2) => {
                self.check_binary_op(env, cursor, bin_op, op1, op2)
            }
            Rvalue::MutRef(place) => {
                // OWNERSHIP SAFETY CHECK
                TyKind::MutRef(env.get_region(place)).intern()
            }
            Rvalue::UnaryOp(un_op, op) => self.check_unary_op(env, *un_op, op),
        }
    }

    fn check_binary_op(
        &self,
        env: &mut TyEnv,
        cursor: &mut Cursor,
        bin_op: &ir::BinOp,
        op1: &Operand,
        op2: &Operand,
    ) -> Ty {
        let ty1 = self.check_operand(env, op1);
        let ty2 = self.check_operand(env, op2);

        match bin_op {
            ir::BinOp::Eq => self.check_eq(BinOp::Eq, ty1, ty2),
            ir::BinOp::Ne => self.check_eq(BinOp::Ne, ty1, ty2),
            ir::BinOp::Add => self.check_arith_op(cursor, BinOp::Add, ty1, ty2),
            ir::BinOp::Sub => self.check_arith_op(cursor, BinOp::Sub, ty1, ty2),
            ir::BinOp::Mul => self.check_arith_op(cursor, BinOp::Mul, ty1, ty2),
            ir::BinOp::Div => self.check_arith_op(cursor, BinOp::Div, ty1, ty2),
            ir::BinOp::Gt => self.check_cmp_op(BinOp::Gt, ty1, ty2),
            ir::BinOp::Lt => self.check_cmp_op(BinOp::Lt, ty1, ty2),
            ir::BinOp::Le => self.check_cmp_op(BinOp::Le, ty1, ty2),
        }
    }

    fn check_arith_op(&self, cursor: &mut Cursor, op: BinOp, ty1: Ty, ty2: Ty) -> Ty {
        let (bty, e1, e2) = match (ty1.kind(), ty2.kind()) {
            (
                TyKind::Refine(BaseTy::Int(int_ty1), e1),
                TyKind::Refine(BaseTy::Int(int_ty2), e2),
            ) => {
                debug_assert_eq!(int_ty1, int_ty2);
                (BaseTy::Int(*int_ty1), e1.clone(), e2.clone())
            }
            (
                TyKind::Refine(BaseTy::Uint(uint_ty1), e1),
                TyKind::Refine(BaseTy::Uint(uint_ty2), e2),
            ) => {
                debug_assert_eq!(uint_ty1, uint_ty2);
                (BaseTy::Uint(*uint_ty1), e1.clone(), e2.clone())
            }
            _ => unreachable!("incompatible types: `{:?}` `{:?}`", ty1, ty2),
        };
        if matches!(op, BinOp::Div) {
            cursor.push_head(ExprKind::BinaryOp(BinOp::Ne, e2.clone(), Expr::zero()).intern());
        }
        TyKind::Refine(bty, ExprKind::BinaryOp(op, e1, e2).intern()).intern()
    }

    fn check_cmp_op(&self, op: BinOp, ty1: Ty, ty2: Ty) -> Ty {
        let (e1, e2) = match (ty1.kind(), ty2.kind()) {
            (
                TyKind::Refine(BaseTy::Int(int_ty1), e1),
                TyKind::Refine(BaseTy::Int(int_ty2), e2),
            ) => {
                debug_assert_eq!(int_ty1, int_ty2);
                (e1.clone(), e2.clone())
            }
            (
                TyKind::Refine(BaseTy::Uint(uint_ty1), e1),
                TyKind::Refine(BaseTy::Uint(uint_ty2), e2),
            ) => {
                debug_assert_eq!(uint_ty1, uint_ty2);
                (e1.clone(), e2.clone())
            }
            _ => unreachable!("incompatible types: `{:?}` `{:?}`", ty1, ty2),
        };
        TyKind::Refine(BaseTy::Bool, ExprKind::BinaryOp(op, e1, e2).intern()).intern()
    }

    fn check_eq(&self, op: BinOp, ty1: Ty, ty2: Ty) -> Ty {
        match (ty1.kind(), ty2.kind()) {
            (TyKind::Refine(bty1, e1), TyKind::Refine(bty2, e2)) => {
                debug_assert_eq!(bty1, bty2);
                TyKind::Refine(
                    BaseTy::Bool,
                    ExprKind::BinaryOp(op, e1.clone(), e2.clone()).intern(),
                )
                .intern()
            }
            _ => unreachable!("incompatible types: `{:?}` `{:?}`", ty1, ty2),
        }
    }

    fn check_unary_op(&self, env: &mut TyEnv, un_op: ir::UnOp, op: &Operand) -> Ty {
        let ty = self.check_operand(env, op);
        match un_op {
            ir::UnOp::Not => match ty.kind() {
                TyKind::Refine(BaseTy::Bool, e) => TyKind::Refine(BaseTy::Bool, e.not()).intern(),
                _ => unreachable!("incompatible type: `{:?}`", ty),
            },
            ir::UnOp::Neg => match ty.kind() {
                TyKind::Refine(BaseTy::Int(int_ty), e) => {
                    TyKind::Refine(BaseTy::Int(*int_ty), e.neg()).intern()
                }
                _ => unreachable!("incompatible type: `{:?}`", ty),
            },
        }
    }

    fn check_operand(&self, env: &mut TyEnv, operand: &Operand) -> Ty {
        match operand {
            Operand::Copy(p) => {
                // OWNERSHIP SAFETY CHECK
                env.lookup_place(p)
            }
            Operand::Move(p) => {
                // OWNERSHIP SAFETY CHECK
                env.move_place(p)
            }
            Operand::Constant(c) => self.check_constant(c),
        }
    }

    fn check_constant(&self, c: &Constant) -> Ty {
        match c {
            Constant::Int(n, int_ty) => {
                let expr = ExprKind::Constant(ty::Constant::from(*n)).intern();
                TyKind::Refine(BaseTy::Int(*int_ty), expr).intern()
            }
            Constant::Uint(n, uint_ty) => {
                let expr = ExprKind::Constant(ty::Constant::from(*n)).intern();
                TyKind::Refine(BaseTy::Uint(*uint_ty), expr).intern()
            }
            Constant::Bool(b) => {
                let expr = ExprKind::Constant(ty::Constant::from(*b)).intern();
                TyKind::Refine(BaseTy::Bool, expr).intern()
            }
        }
    }

    fn report_inference_error(&self, call_source_info: SourceInfo) -> Result<(), ErrorReported> {
        self.sess
            .span_err(call_source_info.span, "inference error at function call");
        Err(ErrorReported)
    }
}
