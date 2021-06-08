use crate::builtins::int;
use crate::builtins::int::PyIntRef;
use crate::builtins::pystr::PyStrRef;
use crate::byteslike::PyBytesLike;
use crate::common::cmp;
use crate::function::OptionalArg;
use crate::iterator;
use crate::utils::Either;
use crate::VirtualMachine;
use crate::{PyObjectRef, PyResult, TypeProtocol};
use int::PyInt;

fn _operator_length_hint(obj: PyObjectRef, default: OptionalArg, vm: &VirtualMachine) -> PyResult {
    let default: usize = default
        .map(|v| {
            if !v.isinstance(&vm.ctx.types.int_type) {
                return Err(vm.new_type_error(format!(
                    "'{}' type cannot be interpreted as an integer",
                    v.class().name
                )));
            }
            int::try_to_primitive(v.payload::<PyInt>().unwrap().as_bigint(), vm)
        })
        .unwrap_or(Ok(0))?;
    iterator::length_hint(vm, obj).map(|v| vm.ctx.new_int(v.unwrap_or(default)))
}

fn _operator_compare_digest(
    a: Either<PyStrRef, PyBytesLike>,
    b: Either<PyStrRef, PyBytesLike>,
    vm: &VirtualMachine,
) -> PyResult<bool> {
    let res = match (a, b) {
        (Either::A(a), Either::A(b)) => {
            if !a.as_str().is_ascii() || !b.as_str().is_ascii() {
                return Err(vm.new_type_error(
                    "comparing strings with non-ASCII characters is not supported".to_owned(),
                ));
            }
            cmp::timing_safe_cmp(a.as_str().as_bytes(), b.as_str().as_bytes())
        }
        (Either::B(a), Either::B(b)) => a.with_ref(|a| b.with_ref(|b| cmp::timing_safe_cmp(a, b))),
        _ => {
            return Err(vm
                .new_type_error("unsupported operand types(s) or combination of types".to_owned()))
        }
    };
    Ok(res)
}

fn _operator_index(obj: PyObjectRef, vm: &VirtualMachine) -> PyResult<PyIntRef> {
    vm.to_index(&obj)
}

pub fn make_module(vm: &VirtualMachine) -> PyObjectRef {
    let ctx = &vm.ctx;
    py_module!(vm, "_operator", {
        "length_hint" => named_function!(ctx, _operator, length_hint),
        "_compare_digest" => named_function!(ctx, _operator, compare_digest),
        "index" => named_function!(ctx, _operator, index),
    })
}
