use crate::lower::{Lower, LowerCtx, LowerResult};

use liquid_rust_lrir::mir::{BasicBlock, BasicBlockData};

use rustc_middle::mir;

impl<'tcx> Lower<'tcx> for mir::BasicBlock {
    type Output = BasicBlock;

    fn lower(&self, _lcx: LowerCtx<'tcx>) -> LowerResult<Self::Output> {
        Ok(*self)
    }
}

impl<'tcx> Lower<'tcx> for mir::BasicBlockData<'tcx> {
    type Output = BasicBlockData;

    fn lower(&self, lcx: LowerCtx<'tcx>) -> LowerResult<Self::Output> {
        let output = BasicBlockData {
            statements: self
                .statements
                .iter()
                .map(|statement| statement.lower(lcx))
                .collect::<LowerResult<Vec<_>>>()?,
            terminator: self.terminator().lower(lcx)?,
        };

        Ok(output)
    }
}
