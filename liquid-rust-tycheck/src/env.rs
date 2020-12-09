use crate::{
    glob_env::GlobEnv,
    glob_variable::GlobVariable,
    result::{TyError, TyErrorKind, TyResult},
    synth::Synth,
};

use liquid_rust_common::index::IndexMap;
use liquid_rust_fixpoint::Emitter;
use liquid_rust_mir::{FuncId, Local, Operand};
use liquid_rust_ty::{BaseTy, LocalVariable, Predicate, Ty, Variable};

pub struct Env<S> {
    func_id: FuncId,
    variables: IndexMap<Local, LocalVariable>,
    types: IndexMap<LocalVariable, Ty>,
    emitter: Emitter<GlobVariable, S>,
}

impl<S> Env<S> {
    pub fn new(
        func_id: FuncId,
        variables: IndexMap<Local, LocalVariable>,
        types: IndexMap<LocalVariable, Ty>,
        emitter: Emitter<GlobVariable, S>,
    ) -> Self {
        Self {
            func_id,
            variables,
            types,
            emitter,
        }
    }

    pub fn emitter(self) -> Emitter<GlobVariable, S> {
        self.emitter
    }

    pub fn get_ty(&self, variable: LocalVariable) -> &Ty {
        self.types.get(variable)
    }

    pub fn resolve_local(&self, local: Local) -> LocalVariable {
        *self.variables.get(local)
    }

    pub fn resolve_operand(&self, operand: &Operand) -> Predicate {
        match operand {
            Operand::Local(local) => Predicate::Var(Variable::Local(self.resolve_local(*local))),
            Operand::Literal(literal) => Predicate::Lit(*literal),
        }
    }

    pub fn annotate_local(&mut self, local: Local, ty: Ty) {
        let variable = self.types.insert(ty.clone());
        *self.variables.get_mut(local) = variable;

        println!("annotated local {} as {}: {}", local, variable, ty);

        match ty {
            Ty::Refined(base_ty, predicate) => {
                let mapper = GlobVariable::mapper(self.func_id);
                self.emitter
                    .add_bind(mapper(variable), base_ty, predicate.map(mapper))
            }
            Ty::Func(..) => todo!(),
        };
    }

    pub fn is_subtype(&mut self, ty1: &Ty, ty2: &Ty, span: S) -> TyResult<S> {
        match (ty1, ty2) {
            // Sub-Base
            (Ty::Refined(base_ty1, predicate1), Ty::Refined(base_ty2, predicate2)) => {
                if base_ty1 == base_ty2 {
                    let env = self.types.iter().map(|(var, _)| var).collect();
                    self.emit_constraint(
                        env,
                        *base_ty1,
                        predicate1.clone(),
                        predicate2.clone(),
                        span,
                    );

                    return Ok(());
                }
            }
            (Ty::Func(_), Ty::Func(_)) => {
                todo!()
            }
            _ => (),
        };

        Err(TyError {
            kind: TyErrorKind::ShapeMismatch {
                expected: ty1.clone(),
                found: ty2.clone(),
            },
            span,
        })
    }

    fn emit_constraint(
        &mut self,
        env: Vec<LocalVariable>,
        base_ty: BaseTy,
        predicate1: Predicate,
        predicate2: Predicate,
        span: S,
    ) {
        let mapper = GlobVariable::mapper(self.func_id);

        self.emitter.add_constraint(
            env.into_iter().map(mapper).collect(),
            base_ty,
            predicate1.map(mapper),
            predicate2.map(mapper),
            span,
        );
    }

    pub fn fork<T>(&mut self, f: impl FnOnce(&mut Self) -> TyResult<T>) -> TyResult<T> {
        let variables = self.variables.clone();
        let result = f(self)?;
        self.variables = variables;
        Ok(result)
    }

    /// Workaround for https://github.com/rust-lang/rust/issues/50238
    pub fn check_syn<T: Synth<S>>(
        &mut self,
        genv: &GlobEnv<S>,
        term: &T,
        ty: &Ty,
        span: S,
    ) -> TyResult<S> {
        let synth_ty = term.synth(genv, self);
        self.is_subtype(&synth_ty, ty, span)
    }
}
