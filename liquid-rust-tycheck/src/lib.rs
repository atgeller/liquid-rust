mod check;
mod env;
mod glob_env;
mod glob_variable;
mod result;
mod synth;

use glob_env::GlobEnv;
use liquid_rust_fixpoint::Emitter;
use liquid_rust_mir::Program;
use liquid_rust_ty::LocalVariable;

pub fn check_program(program: &Program<LocalVariable>) {
    let mut emitter = Emitter::new();

    for (func_id, func) in program.iter() {
        if func.user_ty() {
            emitter = GlobEnv::new(program, func_id).check(emitter);
        }
    }

    emitter.emit().unwrap();
}
