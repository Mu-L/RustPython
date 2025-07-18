pub(crate) use decl::make_module;

#[pymodule(name = "dis")]
mod decl {
    use crate::vm::{
        PyObjectRef, PyRef, PyResult, TryFromObject, VirtualMachine,
        builtins::{PyCode, PyDictRef, PyStrRef},
        bytecode::CodeFlags,
    };

    #[pyfunction]
    fn dis(obj: PyObjectRef, vm: &VirtualMachine) -> PyResult<()> {
        let co = if let Ok(co) = obj.get_attr("__code__", vm) {
            // Method or function:
            PyRef::try_from_object(vm, co)?
        } else if let Ok(co_str) = PyStrRef::try_from_object(vm, obj.clone()) {
            #[cfg(not(feature = "compiler"))]
            {
                let _ = co_str;
                return Err(
                    vm.new_runtime_error("dis.dis() with str argument requires `compiler` feature")
                );
            }
            #[cfg(feature = "compiler")]
            {
                vm.compile(
                    co_str.as_str(),
                    crate::vm::compiler::Mode::Exec,
                    "<dis>".to_owned(),
                )
                .map_err(|err| vm.new_syntax_error(&err, Some(co_str.as_str())))?
            }
        } else {
            PyRef::try_from_object(vm, obj)?
        };
        disassemble(co)
    }

    #[pyfunction]
    fn disassemble(co: PyRef<PyCode>) -> PyResult<()> {
        print!("{}", &co.code);
        Ok(())
    }

    #[pyattr(name = "COMPILER_FLAG_NAMES")]
    fn compiler_flag_names(vm: &VirtualMachine) -> PyDictRef {
        let dict = vm.ctx.new_dict();
        for (name, flag) in CodeFlags::NAME_MAPPING {
            dict.set_item(
                &*vm.new_pyobj(flag.bits()),
                vm.ctx.new_str(*name).into(),
                vm,
            )
            .unwrap();
        }
        dict
    }
}
