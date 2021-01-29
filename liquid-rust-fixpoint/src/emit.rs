use crate::Emitter;

use liquid_rust_common::index::Index;
use liquid_rust_mir::ty::{BaseTy, BinOp, HoleId, LocalVariable, Predicate, UnOp, Variable};

use std::fmt;

pub(crate) struct Writer<'e, T: Emit> {
    pub(crate) emitter: &'e Emitter,
    pub(crate) inner: T,
}

impl<'e, T: Emit> fmt::Display for Writer<'e, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.emit(f, self.emitter)
    }
}

pub(crate) trait Emit {
    fn emit(&self, f: &mut fmt::Formatter<'_>, emitter: &Emitter) -> fmt::Result;
}

impl<T: Emit> Emit for &T {
    fn emit(&self, f: &mut fmt::Formatter<'_>, emitter: &Emitter) -> fmt::Result {
        (*self).emit(f, emitter)
    }
}

impl<T: Emit> Emit for Box<T> {
    fn emit(&self, f: &mut fmt::Formatter<'_>, emitter: &Emitter) -> fmt::Result {
        self.as_ref().emit(f, emitter)
    }
}

impl Emit for BaseTy {
    fn emit(&self, f: &mut fmt::Formatter<'_>, _emitter: &Emitter) -> fmt::Result {
        let ty = match self {
            BaseTy::Unit => "int",
            BaseTy::Int { .. } => "int",
            BaseTy::Bool => "bool",
        };

        write!(f, "{}", ty)
    }
}

impl Emit for Predicate {
    fn emit(&self, f: &mut fmt::Formatter<'_>, emitter: &Emitter) -> fmt::Result {
        match self {
            Predicate::Hole(hole) => hole.id.emit(f, emitter),
            Predicate::Lit(literal) => write!(f, "{}", literal),
            Predicate::Var(variable) => variable.emit(f, emitter),
            Predicate::UnaryOp(un_op, op) => {
                write!(f, "({} {})", emitter.writer(un_op), emitter.writer(op))
            }
            Predicate::BinaryOp(bin_op, op1, op2) => {
                write!(
                    f,
                    "({} {} {})",
                    emitter.writer(op1),
                    emitter.writer(bin_op),
                    emitter.writer(op2)
                )
            }
            Predicate::And(preds) => {
                write!(f, "[")?;
                preds
                    .iter()
                    .map(|pred| emitter.writer(pred))
                    .fold(Ok(true), |first, writer| {
                        if !first? {
                            write!(f, "; ")?;
                        }
                        write!(f, "{}", writer)?;

                        Ok(false)
                    })?;
                write!(f, "]")
            }
        }
    }
}

impl Emit for UnOp {
    fn emit(&self, f: &mut fmt::Formatter<'_>, _emitter: &Emitter) -> fmt::Result {
        match self {
            // Use `not` instead of `!`.
            UnOp::Not { .. } => write!(f, "not"),
            _ => write!(f, "{}", self),
        }
    }
}

impl Emit for BinOp {
    fn emit(&self, f: &mut fmt::Formatter<'_>, _emitter: &Emitter) -> fmt::Result {
        match self {
            // Use `<=>` instead of `==` for booleans.
            BinOp::Eq(BaseTy::Bool) => write!(f, "<=>"),
            _ => write!(f, "{}", self),
        }
    }
}

impl Emit for Variable {
    fn emit(&self, f: &mut fmt::Formatter<'_>, emitter: &Emitter) -> fmt::Result {
        match self {
            Variable::Bound => write!(f, "b"),
            // All arguments should have been projected.
            Variable::Arg(_) => unreachable!(),
            Variable::Local(var) => var.emit(f, emitter),
        }
    }
}

impl Emit for LocalVariable {
    fn emit(&self, f: &mut fmt::Formatter<'_>, emitter: &Emitter) -> fmt::Result {
        if let Some(variable) = emitter.variables.get(self) {
            write!(f, "{}", variable)
        } else {
            panic!("Variable {} is not in scope", self);
        }
    }
}

impl Emit for HoleId {
    fn emit(&self, f: &mut fmt::Formatter<'_>, _emitter: &Emitter) -> fmt::Result {
        write!(f, "$p{}", self.index())
    }
}

impl Emit for usize {
    fn emit(&self, f: &mut fmt::Formatter<'_>, _emitter: &Emitter) -> fmt::Result {
        write!(f, "{}", self)
    }
}
