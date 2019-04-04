use inkwell::{
    builder::Builder,
    context::Context,
    module::{Linkage, Module},
    passes::PassManager,
    types::{BasicType, BasicTypeEnum, FunctionType, PointerType},
    values::{BasicValue, FloatValue, FunctionValue, IntValue, PhiValue, PointerValue},
    AddressSpace, FloatPredicate, IntPredicate,
};
use smallvec::SmallVec;
use wasmer_runtime_core::{
    memory::MemoryType,
    module::ModuleInfo,
    structures::{Map, SliceMap, TypedIndex},
    types::{
        FuncIndex, FuncSig, GlobalIndex, LocalFuncIndex, LocalOrImport, MemoryIndex, SigIndex,
        TableIndex, Type,
    },
};
use wasmparser::{
    BinaryReaderError, CodeSectionReader, LocalsReader, MemoryImmediate, Operator, OperatorsReader,
};

use crate::intrinsics::{CtxType, GlobalCache, Intrinsics, MemoryCache};
use crate::read_info::type_to_type;
use crate::state::{ControlFrame, IfElseState, State};
use crate::trampolines::generate_trampolines;

fn func_sig_to_llvm(context: &Context, intrinsics: &Intrinsics, sig: &FuncSig) -> FunctionType {
    let user_param_types = sig.params().iter().map(|&ty| type_to_llvm(intrinsics, ty));

    let param_types: Vec<_> = std::iter::once(intrinsics.ctx_ptr_ty.as_basic_type_enum())
        .chain(user_param_types)
        .collect();

    match sig.returns() {
        &[] => intrinsics.void_ty.fn_type(&param_types, false),
        &[single_value] => type_to_llvm(intrinsics, single_value).fn_type(&param_types, false),
        returns @ _ => {
            let basic_types: Vec<_> = returns
                .iter()
                .map(|&ty| type_to_llvm(intrinsics, ty))
                .collect();

            context
                .struct_type(&basic_types, false)
                .fn_type(&param_types, false)
        }
    }
}

fn type_to_llvm(intrinsics: &Intrinsics, ty: Type) -> BasicTypeEnum {
    match ty {
        Type::I32 => intrinsics.i32_ty.as_basic_type_enum(),
        Type::I64 => intrinsics.i64_ty.as_basic_type_enum(),
        Type::F32 => intrinsics.f32_ty.as_basic_type_enum(),
        Type::F64 => intrinsics.f64_ty.as_basic_type_enum(),
    }
}

pub fn parse_function_bodies(
    info: &ModuleInfo,
    code_reader: CodeSectionReader,
) -> Result<(Module, Intrinsics), BinaryReaderError> {
    let context = Context::create();
    let module = context.create_module("module");
    let builder = context.create_builder();

    let intrinsics = Intrinsics::declare(&module, &context);

    let personality_func = module.add_function(
        "__gxx_personality_v0",
        intrinsics.i32_ty.fn_type(&[], false),
        Some(Linkage::External),
    );

    let signatures: Map<SigIndex, FunctionType> = info
        .signatures
        .iter()
        .map(|(_, sig)| func_sig_to_llvm(&context, &intrinsics, sig))
        .collect();
    let functions: Map<LocalFuncIndex, FunctionValue> = info
        .func_assoc
        .iter()
        .skip(info.imported_functions.len())
        .map(|(func_index, &sig_index)| {
            let func = module.add_function(
                &format!("fn{}", func_index.index()),
                signatures[sig_index],
                Some(Linkage::External),
            );
            func.set_personality_function(personality_func);
            func
        })
        .collect();

    for (local_func_index, body) in code_reader.into_iter().enumerate() {
        let body = body?;

        let locals_reader = body.get_locals_reader()?;
        let op_reader = body.get_operators_reader()?;

        parse_function(
            &context,
            &builder,
            &intrinsics,
            info,
            &signatures,
            &functions,
            LocalFuncIndex::new(local_func_index),
            locals_reader,
            op_reader,
        )
        .map_err(|e| BinaryReaderError {
            message: e.message,
            offset: local_func_index,
        })?;
    }

    // module.print_to_stderr();

    generate_trampolines(info, &signatures, &module, &context, &builder, &intrinsics);

    let pass_manager = PassManager::create_for_module();
    // pass_manager.add_verifier_pass();
    pass_manager.add_function_inlining_pass();
    pass_manager.add_promote_memory_to_register_pass();
    pass_manager.add_cfg_simplification_pass();
    // pass_manager.add_instruction_combining_pass();
    pass_manager.add_aggressive_inst_combiner_pass();
    pass_manager.add_merged_load_store_motion_pass();
    // pass_manager.add_sccp_pass();
    // pass_manager.add_gvn_pass();
    pass_manager.add_new_gvn_pass();
    pass_manager.add_aggressive_dce_pass();
    pass_manager.run_on_module(&module);

    // module.print_to_stderr();

    Ok((module, intrinsics))
}

fn parse_function(
    context: &Context,
    builder: &Builder,
    intrinsics: &Intrinsics,
    info: &ModuleInfo,
    signatures: &SliceMap<SigIndex, FunctionType>,
    functions: &SliceMap<LocalFuncIndex, FunctionValue>,
    func_index: LocalFuncIndex,
    locals_reader: LocalsReader,
    op_reader: OperatorsReader,
) -> Result<(), BinaryReaderError> {
    let sig_index = info.func_assoc[func_index.convert_up(info)];
    let func_sig = &info.signatures[sig_index];

    let function = functions[func_index];
    let mut state = State::new();
    let entry_block = context.append_basic_block(&function, "entry");

    let return_block = context.append_basic_block(&function, "return");
    builder.position_at_end(&return_block);

    let phis: SmallVec<[PhiValue; 1]> = func_sig
        .returns()
        .iter()
        .map(|&wasmer_ty| type_to_llvm(intrinsics, wasmer_ty))
        .map(|ty| builder.build_phi(ty, &state.var_name()))
        .collect();

    state.push_block(return_block, phis);
    builder.position_at_end(&entry_block);

    let mut locals = Vec::with_capacity(locals_reader.get_count() as usize); // TODO fix capacity

    locals.extend(
        function
            .get_param_iter()
            .skip(1)
            .enumerate()
            .map(|(index, param)| {
                let ty = param.get_type();

                let alloca = builder.build_alloca(ty, &format!("local{}", index));
                builder.build_store(alloca, param);
                alloca
            }),
    );

    let param_len = locals.len();

    let mut local_idx = 0;
    for local in locals_reader.into_iter() {
        let (count, ty) = local?;
        let wasmer_ty = type_to_type(ty)?;
        let ty = type_to_llvm(intrinsics, wasmer_ty);

        let default_value = match wasmer_ty {
            Type::I32 => intrinsics.i32_zero.as_basic_value_enum(),
            Type::I64 => intrinsics.i64_zero.as_basic_value_enum(),
            Type::F32 => intrinsics.f32_zero.as_basic_value_enum(),
            Type::F64 => intrinsics.f64_zero.as_basic_value_enum(),
        };

        for _ in 0..count {
            let alloca = builder.build_alloca(ty, &format!("local{}", param_len + local_idx));

            builder.build_store(alloca, default_value);

            locals.push(alloca);
            local_idx += 1;
        }
    }

    let start_of_code_block = context.append_basic_block(&function, "start_of_code");
    let entry_end_inst = builder.build_unconditional_branch(&start_of_code_block);
    builder.position_at_end(&start_of_code_block);

    let cache_builder = context.create_builder();
    cache_builder.position_before(&entry_end_inst);
    let mut ctx = intrinsics.ctx(info, builder, &function, cache_builder);
    let mut unreachable_depth = 0;

    for op in op_reader {
        let op = op?;
        if !state.reachable {
            match op {
                Operator::Block { ty: _ } | Operator::Loop { ty: _ } | Operator::If { ty: _ } => {
                    unreachable_depth += 1;
                    continue;
                }
                Operator::Else => {
                    if unreachable_depth != 0 {
                        continue;
                    }
                }
                Operator::End => {
                    if unreachable_depth != 0 {
                        unreachable_depth -= 1;
                        continue;
                    }
                }
                _ => {
                    continue;
                }
            }
        }

        match op {
            /***************************
             * Control Flow instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#control-flow-instructions
             ***************************/
            Operator::Block { ty } => {
                let current_block = builder.get_insert_block().ok_or(BinaryReaderError {
                    message: "not currently in a block",
                    offset: -1isize as usize,
                })?;

                let end_block = context.append_basic_block(&function, "end");
                builder.position_at_end(&end_block);

                let phis = if let Ok(wasmer_ty) = type_to_type(ty) {
                    let llvm_ty = type_to_llvm(intrinsics, wasmer_ty);
                    [llvm_ty]
                        .iter()
                        .map(|&ty| builder.build_phi(ty, &state.var_name()))
                        .collect()
                } else {
                    SmallVec::new()
                };

                state.push_block(end_block, phis);
                builder.position_at_end(&current_block);
            }
            Operator::Loop { ty } => {
                let loop_body = context.append_basic_block(&function, "loop_body");
                let loop_next = context.append_basic_block(&function, "loop_outer");

                builder.build_unconditional_branch(&loop_body);

                builder.position_at_end(&loop_next);
                let phis = if let Ok(wasmer_ty) = type_to_type(ty) {
                    let llvm_ty = type_to_llvm(intrinsics, wasmer_ty);
                    [llvm_ty]
                        .iter()
                        .map(|&ty| builder.build_phi(ty, &state.var_name()))
                        .collect()
                } else {
                    SmallVec::new()
                };

                builder.position_at_end(&loop_body);
                state.push_loop(loop_body, loop_next, phis);
            }
            Operator::Br { relative_depth } => {
                let frame = state.frame_at_depth(relative_depth)?;

                let current_block = builder.get_insert_block().ok_or(BinaryReaderError {
                    message: "not currently in a block",
                    offset: -1isize as usize,
                })?;

                let value_len = if frame.is_loop() {
                    0
                } else {
                    frame.phis().len()
                };

                let values = state.peekn(value_len)?;

                // For each result of the block we're branching to,
                // pop a value off the value stack and load it into
                // the corresponding phi.
                for (phi, value) in frame.phis().iter().zip(values.iter()) {
                    phi.add_incoming(&[(value, &current_block)]);
                }

                builder.build_unconditional_branch(frame.br_dest());

                state.popn(value_len)?;
                state.reachable = false;
            }
            Operator::BrIf { relative_depth } => {
                let cond = state.pop1()?;
                let frame = state.frame_at_depth(relative_depth)?;

                let current_block = builder.get_insert_block().ok_or(BinaryReaderError {
                    message: "not currently in a block",
                    offset: -1isize as usize,
                })?;

                let value_len = if frame.is_loop() {
                    0
                } else {
                    frame.phis().len()
                };

                let param_stack = state.peekn(value_len)?;

                for (phi, value) in frame.phis().iter().zip(param_stack.iter()) {
                    phi.add_incoming(&[(value, &current_block)]);
                }

                let else_block = context.append_basic_block(&function, "else");

                let cond_value = builder.build_int_compare(
                    IntPredicate::NE,
                    cond.into_int_value(),
                    intrinsics.i32_zero,
                    &state.var_name(),
                );
                builder.build_conditional_branch(cond_value, frame.br_dest(), &else_block);
                builder.position_at_end(&else_block);
            }
            Operator::BrTable { ref table } => {
                let current_block = builder.get_insert_block().ok_or(BinaryReaderError {
                    message: "not currently in a block",
                    offset: -1isize as usize,
                })?;

                let (label_depths, default_depth) = table.read_table()?;

                let index = state.pop1()?;

                let default_frame = state.frame_at_depth(default_depth)?;

                let args = if default_frame.is_loop() {
                    &[]
                } else {
                    let res_len = default_frame.phis().len();
                    state.peekn(res_len)?
                };

                for (phi, value) in default_frame.phis().iter().zip(args.iter()) {
                    phi.add_incoming(&[(value, &current_block)]);
                }

                let cases: Vec<_> = label_depths
                    .iter()
                    .enumerate()
                    .map(|(case_index, &depth)| {
                        let frame = state.frame_at_depth(depth)?;
                        let case_index_literal =
                            context.i32_type().const_int(case_index as u64, false);

                        for (phi, value) in frame.phis().iter().zip(args.iter()) {
                            phi.add_incoming(&[(value, &current_block)]);
                        }

                        Ok((case_index_literal, frame.br_dest()))
                    })
                    .collect::<Result<_, _>>()?;

                builder.build_switch(index.into_int_value(), default_frame.br_dest(), &cases[..]);

                state.popn(args.len())?;
                state.reachable = false;
            }
            Operator::If { ty } => {
                let current_block = builder.get_insert_block().ok_or(BinaryReaderError {
                    message: "not currently in a block",
                    offset: -1isize as usize,
                })?;
                let if_then_block = context.append_basic_block(&function, "if_then");
                let if_else_block = context.append_basic_block(&function, "if_else");
                let end_block = context.append_basic_block(&function, "if_end");

                let end_phis = {
                    builder.position_at_end(&end_block);

                    let phis = if let Ok(wasmer_ty) = type_to_type(ty) {
                        let llvm_ty = type_to_llvm(intrinsics, wasmer_ty);
                        [llvm_ty]
                            .iter()
                            .map(|&ty| builder.build_phi(ty, &state.var_name()))
                            .collect()
                    } else {
                        SmallVec::new()
                    };

                    builder.position_at_end(&current_block);
                    phis
                };

                let cond = state.pop1()?;

                let cond_value = builder.build_int_compare(
                    IntPredicate::NE,
                    cond.into_int_value(),
                    intrinsics.i32_zero,
                    &state.var_name(),
                );

                builder.build_conditional_branch(cond_value, &if_then_block, &if_else_block);
                builder.position_at_end(&if_then_block);
                state.push_if(if_then_block, if_else_block, end_block, end_phis);
            }
            Operator::Else => {
                if state.reachable {
                    let frame = state.frame_at_depth(0)?;
                    builder.build_unconditional_branch(frame.code_after());
                    let current_block = builder.get_insert_block().ok_or(BinaryReaderError {
                        message: "not currently in a block",
                        offset: -1isize as usize,
                    })?;

                    for phi in frame.phis().to_vec().iter().rev() {
                        let value = state.pop1()?;
                        phi.add_incoming(&[(&value, &current_block)])
                    }
                }

                let (if_else_block, if_else_state) = if let ControlFrame::IfElse {
                    if_else,
                    if_else_state,
                    ..
                } = state.frame_at_depth_mut(0)?
                {
                    (if_else, if_else_state)
                } else {
                    unreachable!()
                };

                *if_else_state = IfElseState::Else;

                builder.position_at_end(if_else_block);
                state.reachable = true;
            }

            Operator::End => {
                let frame = state.pop_frame()?;
                let current_block = builder.get_insert_block().ok_or(BinaryReaderError {
                    message: "not currently in a block",
                    offset: -1isize as usize,
                })?;

                if state.reachable {
                    builder.build_unconditional_branch(frame.code_after());

                    for phi in frame.phis().iter().rev() {
                        let value = state.pop1()?;
                        phi.add_incoming(&[(&value, &current_block)]);
                    }
                }

                if let ControlFrame::IfElse {
                    if_else,
                    next,
                    if_else_state,
                    ..
                } = &frame
                {
                    if let IfElseState::If = if_else_state {
                        builder.position_at_end(if_else);
                        builder.build_unconditional_branch(next);
                    }
                }

                builder.position_at_end(frame.code_after());
                state.reset_stack(&frame);

                state.reachable = true;

                // Push each phi value to the value stack.
                for phi in frame.phis() {
                    if phi.count_incoming() != 0 {
                        state.push1(phi.as_basic_value());
                    } else {
                        let basic_ty = phi.as_basic_value().get_type();
                        let placeholder_value = match basic_ty {
                            BasicTypeEnum::IntType(int_ty) => {
                                int_ty.const_int(0, false).as_basic_value_enum()
                            }
                            BasicTypeEnum::FloatType(float_ty) => {
                                float_ty.const_float(0.0).as_basic_value_enum()
                            }
                            _ => unimplemented!(),
                        };
                        state.push1(placeholder_value);
                        phi.as_instruction().erase_from_basic_block();
                    }
                }
            }
            Operator::Return => {
                let frame = state.outermost_frame()?;
                let current_block = builder.get_insert_block().ok_or(BinaryReaderError {
                    message: "not currently in a block",
                    offset: -1isize as usize,
                })?;

                builder.build_unconditional_branch(frame.br_dest());

                let phis = frame.phis().to_vec();

                for phi in phis.iter() {
                    let arg = state.pop1()?;
                    phi.add_incoming(&[(&arg, &current_block)]);
                }

                state.reachable = false;
            }

            Operator::Unreachable => {
                // Emit an unreachable instruction.
                // If llvm cannot prove that this is never touched,
                // it will emit a `ud2` instruction on x86_64 arches.

                builder.build_call(
                    intrinsics.throw_trap,
                    &[intrinsics.trap_unreachable],
                    "throw",
                );
                builder.build_unreachable();

                state.reachable = false;
            }

            /***************************
             * Basic instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#basic-instructions
             ***************************/
            Operator::Nop => {
                // Do nothing.
            }
            Operator::Drop => {
                state.pop1()?;
            }

            // Generate const values.
            Operator::I32Const { value } => {
                let i = intrinsics.i32_ty.const_int(value as u64, false);
                state.push1(i);
            }
            Operator::I64Const { value } => {
                let i = intrinsics.i64_ty.const_int(value as u64, false);
                state.push1(i);
            }
            Operator::F32Const { value } => {
                let bits = intrinsics.i32_ty.const_int(value.bits() as u64, false);
                let space =
                    builder.build_alloca(intrinsics.f32_ty.as_basic_type_enum(), "const_space");
                let i32_space =
                    builder.build_pointer_cast(space, intrinsics.i32_ptr_ty, "i32_space");
                builder.build_store(i32_space, bits);
                let f = builder.build_load(space, "f");
                state.push1(f);
            }
            Operator::F64Const { value } => {
                let bits = intrinsics.i64_ty.const_int(value.bits(), false);
                let space =
                    builder.build_alloca(intrinsics.f64_ty.as_basic_type_enum(), "const_space");
                let i64_space =
                    builder.build_pointer_cast(space, intrinsics.i64_ptr_ty, "i32_space");
                builder.build_store(i64_space, bits);
                let f = builder.build_load(space, "f");
                state.push1(f);
            }

            // Operate on locals.
            Operator::GetLocal { local_index } => {
                let pointer_value = locals[local_index as usize];
                let v = builder.build_load(pointer_value, &state.var_name());
                state.push1(v);
            }
            Operator::SetLocal { local_index } => {
                let pointer_value = locals[local_index as usize];
                let v = state.pop1()?;
                builder.build_store(pointer_value, v);
            }
            Operator::TeeLocal { local_index } => {
                let pointer_value = locals[local_index as usize];
                let v = state.peek1()?;
                builder.build_store(pointer_value, v);
            }

            Operator::GetGlobal { global_index } => {
                let index = GlobalIndex::new(global_index as usize);
                let global_cache = ctx.global_cache(index);
                match global_cache {
                    GlobalCache::Const { value } => {
                        state.push1(value);
                    }
                    GlobalCache::Mut { ptr_to_value } => {
                        let value = builder.build_load(ptr_to_value, "global_value");
                        state.push1(value);
                    }
                }
            }
            Operator::SetGlobal { global_index } => {
                let value = state.pop1()?;
                let index = GlobalIndex::new(global_index as usize);
                let global_cache = ctx.global_cache(index);
                match global_cache {
                    GlobalCache::Mut { ptr_to_value } => {
                        builder.build_store(ptr_to_value, value);
                    }
                    GlobalCache::Const { value: _ } => {
                        unreachable!("cannot set non-mutable globals")
                    }
                }
            }

            Operator::Select => {
                let (v1, v2, cond) = state.pop3()?;
                let cond_value = builder.build_int_compare(
                    IntPredicate::NE,
                    cond.into_int_value(),
                    intrinsics.i32_zero,
                    &state.var_name(),
                );
                let res = builder.build_select(cond_value, v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::Call { function_index } => {
                let func_index = FuncIndex::new(function_index as usize);
                let sigindex = info.func_assoc[func_index];
                let llvm_sig = signatures[sigindex];
                let func_sig = &info.signatures[sigindex];

                let call_site = match func_index.local_or_import(info) {
                    LocalOrImport::Local(local_func_index) => {
                        let params: Vec<_> = [ctx.basic()]
                            .iter()
                            .chain(state.peekn(func_sig.params().len())?.iter())
                            .map(|v| *v)
                            .collect();

                        let func_ptr = ctx.local_func(local_func_index, llvm_sig);

                        builder.build_call(func_ptr, &params, &state.var_name())
                    }
                    LocalOrImport::Import(import_func_index) => {
                        let (func_ptr_untyped, ctx_ptr) = ctx.imported_func(import_func_index);
                        let params: Vec<_> = [ctx_ptr.as_basic_value_enum()]
                            .iter()
                            .chain(state.peekn(func_sig.params().len())?.iter())
                            .map(|v| *v)
                            .collect();

                        let func_ptr_ty = llvm_sig.ptr_type(AddressSpace::Generic);

                        let func_ptr = builder.build_pointer_cast(
                            func_ptr_untyped,
                            func_ptr_ty,
                            "typed_func_ptr",
                        );

                        builder.build_call(func_ptr, &params, &state.var_name())
                    }
                };

                state.popn(func_sig.params().len())?;

                if let Some(basic_value) = call_site.try_as_basic_value().left() {
                    match func_sig.returns().len() {
                        1 => state.push1(basic_value),
                        count @ _ => {
                            // This is a multi-value return.
                            let struct_value = basic_value.into_struct_value();
                            for i in 0..(count as u32) {
                                let value = builder
                                    .build_extract_value(struct_value, i, &state.var_name())
                                    .unwrap();
                                state.push1(value);
                            }
                        }
                    }
                }
            }
            Operator::CallIndirect { index, table_index } => {
                let sig_index = SigIndex::new(index as usize);
                let expected_dynamic_sigindex = ctx.dynamic_sigindex(sig_index);
                let (table_base, table_bound) = ctx.table(TableIndex::new(table_index as usize));
                let func_index = state.pop1()?.into_int_value();

                // We assume the table has the `anyfunc` element type.
                let casted_table_base = builder.build_pointer_cast(
                    table_base,
                    intrinsics.anyfunc_ty.ptr_type(AddressSpace::Generic),
                    "casted_table_base",
                );

                let anyfunc_struct_ptr = unsafe {
                    builder.build_in_bounds_gep(
                        casted_table_base,
                        &[func_index],
                        "anyfunc_struct_ptr",
                    )
                };

                // Load things from the anyfunc data structure.
                let (func_ptr, ctx_ptr, found_dynamic_sigindex) = unsafe {
                    (
                        builder
                            .build_load(
                                builder.build_struct_gep(anyfunc_struct_ptr, 0, "func_ptr_ptr"),
                                "func_ptr",
                            )
                            .into_pointer_value(),
                        builder.build_load(
                            builder.build_struct_gep(anyfunc_struct_ptr, 1, "ctx_ptr_ptr"),
                            "ctx_ptr",
                        ),
                        builder
                            .build_load(
                                builder.build_struct_gep(anyfunc_struct_ptr, 2, "sigindex_ptr"),
                                "sigindex",
                            )
                            .into_int_value(),
                    )
                };

                let truncated_table_bounds = builder.build_int_truncate(
                    table_bound,
                    intrinsics.i32_ty,
                    "truncated_table_bounds",
                );

                // First, check if the index is outside of the table bounds.
                let index_in_bounds = builder.build_int_compare(
                    IntPredicate::ULT,
                    func_index,
                    truncated_table_bounds,
                    "index_in_bounds",
                );

                let index_in_bounds = builder
                    .build_call(
                        intrinsics.expect_i1,
                        &[
                            index_in_bounds.as_basic_value_enum(),
                            intrinsics.i1_ty.const_int(1, false).as_basic_value_enum(),
                        ],
                        "index_in_bounds_expect",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value();

                let in_bounds_continue_block =
                    context.append_basic_block(&function, "in_bounds_continue_block");
                let not_in_bounds_block =
                    context.append_basic_block(&function, "not_in_bounds_block");
                builder.build_conditional_branch(
                    index_in_bounds,
                    &in_bounds_continue_block,
                    &not_in_bounds_block,
                );
                builder.position_at_end(&not_in_bounds_block);
                builder.build_call(
                    intrinsics.throw_trap,
                    &[intrinsics.trap_call_indirect_oob],
                    "throw",
                );
                builder.build_unreachable();
                builder.position_at_end(&in_bounds_continue_block);

                // Next, check if the signature id is correct.

                let sigindices_equal = builder.build_int_compare(
                    IntPredicate::EQ,
                    expected_dynamic_sigindex,
                    found_dynamic_sigindex,
                    "sigindices_equal",
                );

                // Tell llvm that `expected_dynamic_sigindex` should equal `found_dynamic_sigindex`.
                let sigindices_equal = builder
                    .build_call(
                        intrinsics.expect_i1,
                        &[
                            sigindices_equal.as_basic_value_enum(),
                            intrinsics.i1_ty.const_int(1, false).as_basic_value_enum(),
                        ],
                        "sigindices_equal_expect",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value();

                let continue_block = context.append_basic_block(&function, "continue_block");
                let sigindices_notequal_block =
                    context.append_basic_block(&function, "sigindices_notequal_block");
                builder.build_conditional_branch(
                    sigindices_equal,
                    &continue_block,
                    &sigindices_notequal_block,
                );

                builder.position_at_end(&sigindices_notequal_block);
                builder.build_call(
                    intrinsics.throw_trap,
                    &[intrinsics.trap_call_indirect_sig],
                    "throw",
                );
                builder.build_unreachable();
                builder.position_at_end(&continue_block);

                let wasmer_fn_sig = &info.signatures[sig_index];
                let fn_ty = signatures[sig_index];

                let pushed_args = state.popn_save(wasmer_fn_sig.params().len())?;

                let args: Vec<_> = std::iter::once(ctx_ptr)
                    .chain(pushed_args.into_iter())
                    .collect();

                let typed_func_ptr = builder.build_pointer_cast(
                    func_ptr,
                    fn_ty.ptr_type(AddressSpace::Generic),
                    "typed_func_ptr",
                );

                let call_site = builder.build_call(typed_func_ptr, &args, "indirect_call");

                match wasmer_fn_sig.returns() {
                    [] => {}
                    [_] => {
                        let value = call_site.try_as_basic_value().left().unwrap();
                        state.push1(value);
                    }
                    _ => unimplemented!("multi-value returns"),
                }
            }

            /***************************
             * Integer Arithmetic instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#integer-arithmetic-instructions
             ***************************/
            Operator::I32Add | Operator::I64Add => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_int_add(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32Sub | Operator::I64Sub => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_int_sub(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32Mul | Operator::I64Mul => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_int_mul(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32DivS | Operator::I64DivS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());

                trap_if_zero_or_overflow(builder, intrinsics, context, &function, v1, v2);

                let res = builder.build_int_signed_div(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32DivU | Operator::I64DivU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());

                trap_if_zero(builder, intrinsics, context, &function, v2);

                let res = builder.build_int_unsigned_div(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32RemS | Operator::I64RemS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());

                trap_if_zero(builder, intrinsics, context, &function, v2);

                let res = builder.build_int_signed_rem(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32RemU | Operator::I64RemU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());

                trap_if_zero(builder, intrinsics, context, &function, v2);

                let res = builder.build_int_unsigned_rem(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32And | Operator::I64And => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_and(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32Or | Operator::I64Or => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_or(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32Xor | Operator::I64Xor => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_xor(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32Shl | Operator::I64Shl => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_left_shift(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32ShrS | Operator::I64ShrS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_right_shift(v1, v2, true, &state.var_name());
                state.push1(res);
            }
            Operator::I32ShrU | Operator::I64ShrU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_right_shift(v1, v2, false, &state.var_name());
                state.push1(res);
            }
            Operator::I32Rotl => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let lhs = builder.build_left_shift(v1, v2, &state.var_name());
                let rhs = {
                    let int_width = intrinsics.i32_ty.const_int(32 as u64, false);
                    let rhs = builder.build_int_sub(int_width, v2, &state.var_name());
                    builder.build_right_shift(v1, rhs, false, &state.var_name())
                };
                let res = builder.build_or(lhs, rhs, &state.var_name());
                state.push1(res);
            }
            Operator::I64Rotl => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let lhs = builder.build_left_shift(v1, v2, &state.var_name());
                let rhs = {
                    let int_width = intrinsics.i64_ty.const_int(64 as u64, false);
                    let rhs = builder.build_int_sub(int_width, v2, &state.var_name());
                    builder.build_right_shift(v1, rhs, false, &state.var_name())
                };
                let res = builder.build_or(lhs, rhs, &state.var_name());
                state.push1(res);
            }
            Operator::I32Rotr => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let lhs = builder.build_right_shift(v1, v2, false, &state.var_name());
                let rhs = {
                    let int_width = intrinsics.i32_ty.const_int(32 as u64, false);
                    let rhs = builder.build_int_sub(int_width, v2, &state.var_name());
                    builder.build_left_shift(v1, rhs, &state.var_name())
                };
                let res = builder.build_or(lhs, rhs, &state.var_name());
                state.push1(res);
            }
            Operator::I64Rotr => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let lhs = builder.build_right_shift(v1, v2, false, &state.var_name());
                let rhs = {
                    let int_width = intrinsics.i64_ty.const_int(64 as u64, false);
                    let rhs = builder.build_int_sub(int_width, v2, &state.var_name());
                    builder.build_left_shift(v1, rhs, &state.var_name())
                };
                let res = builder.build_or(lhs, rhs, &state.var_name());
                state.push1(res);
            }
            Operator::I32Clz => {
                let input = state.pop1()?;
                let ensure_defined_zero = intrinsics
                    .i1_ty
                    .const_int(1 as u64, false)
                    .as_basic_value_enum();
                let res = builder
                    .build_call(
                        intrinsics.ctlz_i32,
                        &[input, ensure_defined_zero],
                        &state.var_name(),
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::I64Clz => {
                let input = state.pop1()?;
                let ensure_defined_zero = intrinsics
                    .i1_ty
                    .const_int(1 as u64, false)
                    .as_basic_value_enum();
                let res = builder
                    .build_call(
                        intrinsics.ctlz_i64,
                        &[input, ensure_defined_zero],
                        &state.var_name(),
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::I32Ctz => {
                let input = state.pop1()?;
                let ensure_defined_zero = intrinsics
                    .i1_ty
                    .const_int(1 as u64, false)
                    .as_basic_value_enum();
                let res = builder
                    .build_call(
                        intrinsics.cttz_i32,
                        &[input, ensure_defined_zero],
                        &state.var_name(),
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::I64Ctz => {
                let input = state.pop1()?;
                let ensure_defined_zero = intrinsics
                    .i1_ty
                    .const_int(1 as u64, false)
                    .as_basic_value_enum();
                let res = builder
                    .build_call(
                        intrinsics.cttz_i64,
                        &[input, ensure_defined_zero],
                        &state.var_name(),
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::I32Popcnt => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.ctpop_i32, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::I64Popcnt => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.ctpop_i64, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::I32Eqz => {
                let input = state.pop1()?.into_int_value();
                let cond = builder.build_int_compare(
                    IntPredicate::EQ,
                    input,
                    intrinsics.i32_zero,
                    &state.var_name(),
                );
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64Eqz => {
                let input = state.pop1()?.into_int_value();
                let cond = builder.build_int_compare(
                    IntPredicate::EQ,
                    input,
                    intrinsics.i64_zero,
                    &state.var_name(),
                );
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }

            /***************************
             * Floating-Point Arithmetic instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#floating-point-arithmetic-instructions
             ***************************/
            Operator::F32Add | Operator::F64Add => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = builder.build_float_add(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::F32Sub | Operator::F64Sub => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = builder.build_float_sub(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::F32Mul | Operator::F64Mul => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = builder.build_float_mul(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::F32Div | Operator::F64Div => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = builder.build_float_div(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::F32Sqrt => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.sqrt_f32, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Sqrt => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.sqrt_f64, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F32Min => {
                let (v1, v2) = state.pop2()?;
                let res = builder
                    .build_call(intrinsics.minimum_f32, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Min => {
                let (v1, v2) = state.pop2()?;
                let res = builder
                    .build_call(intrinsics.minimum_f64, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F32Max => {
                let (v1, v2) = state.pop2()?;
                let res = builder
                    .build_call(intrinsics.maximum_f32, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Max => {
                let (v1, v2) = state.pop2()?;
                let res = builder
                    .build_call(intrinsics.maximum_f64, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F32Ceil => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.ceil_f32, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Ceil => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.ceil_f64, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F32Floor => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.floor_f32, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Floor => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.floor_f64, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F32Trunc => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.trunc_f32, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Trunc => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.trunc_f64, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F32Nearest => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.nearbyint_f32, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Nearest => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.nearbyint_f64, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F32Abs => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.fabs_f32, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Abs => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.fabs_f64, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F32Neg | Operator::F64Neg => {
                let input = state.pop1()?.into_float_value();
                let res = builder.build_float_neg(input, &state.var_name());
                state.push1(res);
            }
            Operator::F32Copysign => {
                let (mag, sgn) = state.pop2()?;
                let res = builder
                    .build_call(intrinsics.copysign_f32, &[mag, sgn], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Copysign => {
                let (msg, sgn) = state.pop2()?;
                let res = builder
                    .build_call(intrinsics.copysign_f64, &[msg, sgn], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }

            /***************************
             * Integer Comparison instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#integer-comparison-instructions
             ***************************/
            Operator::I32Eq | Operator::I64Eq => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::EQ, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32Ne | Operator::I64Ne => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::NE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32LtS | Operator::I64LtS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::SLT, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32LtU | Operator::I64LtU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::ULT, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32LeS | Operator::I64LeS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::SLE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32LeU | Operator::I64LeU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::ULE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32GtS | Operator::I64GtS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::SGT, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32GtU | Operator::I64GtU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::UGT, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32GeS | Operator::I64GeS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::SGE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32GeU | Operator::I64GeU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::UGE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }

            /***************************
             * Floating-Point Comparison instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#floating-point-comparison-instructions
             ***************************/
            Operator::F32Eq | Operator::F64Eq => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond =
                    builder.build_float_compare(FloatPredicate::OEQ, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32Ne | Operator::F64Ne => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond =
                    builder.build_float_compare(FloatPredicate::UNE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32Lt | Operator::F64Lt => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond =
                    builder.build_float_compare(FloatPredicate::OLT, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32Le | Operator::F64Le => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond =
                    builder.build_float_compare(FloatPredicate::OLE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32Gt | Operator::F64Gt => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond =
                    builder.build_float_compare(FloatPredicate::OGT, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32Ge | Operator::F64Ge => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond =
                    builder.build_float_compare(FloatPredicate::OGE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }

            /***************************
             * Conversion instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#conversion-instructions
             ***************************/
            Operator::I32WrapI64 => {
                let v1 = state.pop1()?.into_int_value();
                let res = builder.build_int_truncate(v1, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64ExtendSI32 => {
                let v1 = state.pop1()?.into_int_value();
                let res = builder.build_int_s_extend(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64ExtendUI32 => {
                let v1 = state.pop1()?.into_int_value();
                let res = builder.build_int_z_extend(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32TruncSF32 => {
                let v1 = state.pop1()?.into_float_value();
                trap_if_not_representatable_as_int(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    -2147483904.0,
                    2147483648.0,
                    v1,
                );
                let res =
                    builder.build_float_to_signed_int(v1, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32TruncSF64 => {
                let v1 = state.pop1()?.into_float_value();
                trap_if_not_representatable_as_int(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    -2147483649.0,
                    2147483648.0,
                    v1,
                );
                let res =
                    builder.build_float_to_signed_int(v1, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32TruncSSatF32 | Operator::I32TruncSSatF64 => {
                let v1 = state.pop1()?.into_float_value();
                let res =
                    builder.build_float_to_signed_int(v1, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64TruncSF32 => {
                let v1 = state.pop1()?.into_float_value();
                trap_if_not_representatable_as_int(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    -9223373136366403584.0,
                    9223372036854775808.0,
                    v1,
                );
                let res =
                    builder.build_float_to_signed_int(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64TruncSF64 => {
                let v1 = state.pop1()?.into_float_value();
                trap_if_not_representatable_as_int(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    -9223372036854777856.0,
                    9223372036854775808.0,
                    v1,
                );
                let res =
                    builder.build_float_to_signed_int(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64TruncSSatF32 | Operator::I64TruncSSatF64 => {
                let v1 = state.pop1()?.into_float_value();
                let res =
                    builder.build_float_to_signed_int(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32TruncUF32 => {
                let v1 = state.pop1()?.into_float_value();
                trap_if_not_representatable_as_int(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    -1.0,
                    4294967296.0,
                    v1,
                );
                let res =
                    builder.build_float_to_unsigned_int(v1, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32TruncUF64 => {
                let v1 = state.pop1()?.into_float_value();
                trap_if_not_representatable_as_int(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    -1.0,
                    4294967296.0,
                    v1,
                );
                let res =
                    builder.build_float_to_unsigned_int(v1, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32TruncUSatF32 | Operator::I32TruncUSatF64 => {
                let v1 = state.pop1()?.into_float_value();
                let res =
                    builder.build_float_to_unsigned_int(v1, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64TruncUF32 => {
                let v1 = state.pop1()?.into_float_value();
                trap_if_not_representatable_as_int(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    -1.0,
                    18446744073709551616.0,
                    v1,
                );
                let res =
                    builder.build_float_to_unsigned_int(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64TruncUF64 => {
                let v1 = state.pop1()?.into_float_value();
                trap_if_not_representatable_as_int(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    -1.0,
                    18446744073709551616.0,
                    v1,
                );
                let res =
                    builder.build_float_to_unsigned_int(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64TruncUSatF32 | Operator::I64TruncUSatF64 => {
                let v1 = state.pop1()?.into_float_value();
                let res =
                    builder.build_float_to_unsigned_int(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32DemoteF64 => {
                let v1 = state.pop1()?.into_float_value();
                let res = builder.build_float_trunc(v1, intrinsics.f32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F64PromoteF32 => {
                let v1 = state.pop1()?.into_float_value();
                let res = builder.build_float_ext(v1, intrinsics.f64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32ConvertSI32 | Operator::F32ConvertSI64 => {
                let v1 = state.pop1()?.into_int_value();
                let res =
                    builder.build_signed_int_to_float(v1, intrinsics.f32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F64ConvertSI32 | Operator::F64ConvertSI64 => {
                let v1 = state.pop1()?.into_int_value();
                let res =
                    builder.build_signed_int_to_float(v1, intrinsics.f64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32ConvertUI32 | Operator::F32ConvertUI64 => {
                let v1 = state.pop1()?.into_int_value();
                let res =
                    builder.build_unsigned_int_to_float(v1, intrinsics.f32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F64ConvertUI32 | Operator::F64ConvertUI64 => {
                let v1 = state.pop1()?.into_int_value();
                let res =
                    builder.build_unsigned_int_to_float(v1, intrinsics.f64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32ReinterpretF32 => {
                let v = state.pop1()?;
                let space =
                    builder.build_alloca(intrinsics.i32_ty.as_basic_type_enum(), &state.var_name());
                let f32_space =
                    builder.build_pointer_cast(space, intrinsics.f32_ptr_ty, &state.var_name());
                builder.build_store(f32_space, v);
                let int = builder.build_load(space, &state.var_name());
                state.push1(int);
            }
            Operator::I64ReinterpretF64 => {
                let v = state.pop1()?;
                let space =
                    builder.build_alloca(intrinsics.i64_ty.as_basic_type_enum(), &state.var_name());
                let f64_space =
                    builder.build_pointer_cast(space, intrinsics.f64_ptr_ty, &state.var_name());
                builder.build_store(f64_space, v);
                let int = builder.build_load(space, &state.var_name());
                state.push1(int);
            }
            Operator::F32ReinterpretI32 => {
                let v = state.pop1()?;
                let space =
                    builder.build_alloca(intrinsics.f32_ty.as_basic_type_enum(), &state.var_name());
                let i32_space =
                    builder.build_pointer_cast(space, intrinsics.i32_ptr_ty, &state.var_name());
                builder.build_store(i32_space, v);
                let f = builder.build_load(space, &state.var_name());
                state.push1(f);
            }
            Operator::F64ReinterpretI64 => {
                let v = state.pop1()?;
                let space =
                    builder.build_alloca(intrinsics.f64_ty.as_basic_type_enum(), &state.var_name());
                let i64_space =
                    builder.build_pointer_cast(space, intrinsics.i64_ptr_ty, &state.var_name());
                builder.build_store(i64_space, v);
                let f = builder.build_load(space, &state.var_name());
                state.push1(f);
            }

            /***************************
             * Sign-extension operators.
             * https://github.com/WebAssembly/sign-extension-ops/blob/master/proposals/sign-extension-ops/Overview.md
             ***************************/
            Operator::I32Extend8S => {
                let value = state.pop1()?.into_int_value();
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i8_ty, &state.var_name());
                let extended_value =
                    builder.build_int_s_extend(narrow_value, intrinsics.i32_ty, &state.var_name());
                state.push1(extended_value);
            }
            Operator::I32Extend16S => {
                let value = state.pop1()?.into_int_value();
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i16_ty, &state.var_name());
                let extended_value =
                    builder.build_int_s_extend(narrow_value, intrinsics.i32_ty, &state.var_name());
                state.push1(extended_value);
            }
            Operator::I64Extend8S => {
                let value = state.pop1()?.into_int_value();
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i8_ty, &state.var_name());
                let extended_value =
                    builder.build_int_s_extend(narrow_value, intrinsics.i64_ty, &state.var_name());
                state.push1(extended_value);
            }
            Operator::I64Extend16S => {
                let value = state.pop1()?.into_int_value();
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i16_ty, &state.var_name());
                let extended_value =
                    builder.build_int_s_extend(narrow_value, intrinsics.i64_ty, &state.var_name());
                state.push1(extended_value);
            }
            Operator::I64Extend32S => {
                let value = state.pop1()?.into_int_value();
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i32_ty, &state.var_name());
                let extended_value =
                    builder.build_int_s_extend(narrow_value, intrinsics.i64_ty, &state.var_name());
                state.push1(extended_value);
            }

            /***************************
             * Load and Store instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#load-and-store-instructions
             ***************************/
            Operator::I32Load { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                )?;
                let result = builder.build_load(effective_address, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i64_ptr_ty,
                )?;
                let result = builder.build_load(effective_address, &state.var_name());
                state.push1(result);
            }
            Operator::F32Load { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.f32_ptr_ty,
                )?;
                let result = builder.build_load(effective_address, &state.var_name());
                state.push1(result);
            }
            Operator::F64Load { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.f64_ptr_ty,
                )?;
                let result = builder.build_load(effective_address, &state.var_name());
                state.push1(result);
            }

            Operator::I32Store { memarg } => {
                let value = state.pop1()?;
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                )?;
                builder.build_store(effective_address, value);
            }
            Operator::I64Store { memarg } => {
                let value = state.pop1()?;
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i64_ptr_ty,
                )?;
                builder.build_store(effective_address, value);
            }
            Operator::F32Store { memarg } => {
                let value = state.pop1()?;
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.f32_ptr_ty,
                )?;
                builder.build_store(effective_address, value);
            }
            Operator::F64Store { memarg } => {
                let value = state.pop1()?;
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.f64_ptr_ty,
                )?;
                builder.build_store(effective_address, value);
            }

            Operator::I32Load8S { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i8_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_s_extend(narrow_result, intrinsics.i32_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I32Load16S { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i16_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_s_extend(narrow_result, intrinsics.i32_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load8S { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i8_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_s_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load16S { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i16_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_s_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load32S { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_s_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }

            Operator::I32Load8U { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i8_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_z_extend(narrow_result, intrinsics.i32_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I32Load16U { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i16_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_z_extend(narrow_result, intrinsics.i32_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load8U { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i8_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_z_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load16U { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i16_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_z_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load32U { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_z_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }

            Operator::I32Store8 { memarg } | Operator::I64Store8 { memarg } => {
                let value = state.pop1()?.into_int_value();
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i8_ptr_ty,
                )?;
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i8_ty, &state.var_name());
                builder.build_store(effective_address, narrow_value);
            }
            Operator::I32Store16 { memarg } | Operator::I64Store16 { memarg } => {
                let value = state.pop1()?.into_int_value();
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i16_ptr_ty,
                )?;
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i16_ty, &state.var_name());
                builder.build_store(effective_address, narrow_value);
            }
            Operator::I64Store32 { memarg } => {
                let value = state.pop1()?.into_int_value();
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                )?;
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i32_ty, &state.var_name());
                builder.build_store(effective_address, narrow_value);
            }

            Operator::MemoryGrow { reserved } => {
                let memory_index = MemoryIndex::new(reserved as usize);
                let func_value = match memory_index.local_or_import(info) {
                    LocalOrImport::Local(local_mem_index) => {
                        let mem_desc = &info.memories[local_mem_index];
                        match mem_desc.memory_type() {
                            MemoryType::Dynamic => intrinsics.memory_grow_dynamic_local,
                            MemoryType::Static => intrinsics.memory_grow_static_local,
                            MemoryType::SharedStatic => intrinsics.memory_grow_shared_local,
                        }
                    }
                    LocalOrImport::Import(import_mem_index) => {
                        let mem_desc = &info.imported_memories[import_mem_index].1;
                        match mem_desc.memory_type() {
                            MemoryType::Dynamic => intrinsics.memory_grow_dynamic_import,
                            MemoryType::Static => intrinsics.memory_grow_static_import,
                            MemoryType::SharedStatic => intrinsics.memory_grow_shared_import,
                        }
                    }
                };

                let memory_index_const = intrinsics
                    .i32_ty
                    .const_int(reserved as u64, false)
                    .as_basic_value_enum();
                let delta = state.pop1()?;

                let result = builder.build_call(
                    func_value,
                    &[ctx.basic(), memory_index_const, delta],
                    &state.var_name(),
                );
                state.push1(result.try_as_basic_value().left().unwrap());
            }
            Operator::MemorySize { reserved } => {
                let memory_index = MemoryIndex::new(reserved as usize);
                let func_value = match memory_index.local_or_import(info) {
                    LocalOrImport::Local(local_mem_index) => {
                        let mem_desc = &info.memories[local_mem_index];
                        match mem_desc.memory_type() {
                            MemoryType::Dynamic => intrinsics.memory_size_dynamic_local,
                            MemoryType::Static => intrinsics.memory_size_static_local,
                            MemoryType::SharedStatic => intrinsics.memory_size_shared_local,
                        }
                    }
                    LocalOrImport::Import(import_mem_index) => {
                        let mem_desc = &info.imported_memories[import_mem_index].1;
                        match mem_desc.memory_type() {
                            MemoryType::Dynamic => intrinsics.memory_size_dynamic_import,
                            MemoryType::Static => intrinsics.memory_size_static_import,
                            MemoryType::SharedStatic => intrinsics.memory_size_shared_import,
                        }
                    }
                };

                let memory_index_const = intrinsics
                    .i32_ty
                    .const_int(reserved as u64, false)
                    .as_basic_value_enum();
                let result = builder.build_call(
                    func_value,
                    &[ctx.basic(), memory_index_const],
                    &state.var_name(),
                );
                state.push1(result.try_as_basic_value().left().unwrap());
            }
            op @ _ => {
                unimplemented!("{:?}", op);
            }
        }
    }

    let results = state.popn_save(func_sig.returns().len())?;

    match results.as_slice() {
        [] => {
            builder.build_return(None);
        }
        [one_value] => {
            builder.build_return(Some(one_value));
        }
        _ => {
            // let struct_ty = llvm_sig.get_return_type().as_struct_type();
            // let ret_struct = struct_ty.const_zero();
            unimplemented!("multi-value returns not yet implemented")
        }
    }

    Ok(())
}

fn trap_if_not_representatable_as_int(
    builder: &Builder,
    intrinsics: &Intrinsics,
    context: &Context,
    function: &FunctionValue,
    lower_bounds: f64,
    upper_bound: f64,
    value: FloatValue,
) {
    enum FloatSize {
        Bits32,
        Bits64,
    }

    let failure_block = context.append_basic_block(function, "conversion_failure_block");
    let continue_block = context.append_basic_block(function, "conversion_success_block");

    let float_ty = value.get_type();
    let (int_ty, float_ptr_ty, float_size) = if float_ty == intrinsics.f32_ty {
        (intrinsics.i32_ty, intrinsics.f32_ptr_ty, FloatSize::Bits32)
    } else if float_ty == intrinsics.f64_ty {
        (intrinsics.i64_ty, intrinsics.f64_ptr_ty, FloatSize::Bits64)
    } else {
        unreachable!()
    };

    let (exponent, invalid_exponent) = {
        let float_bits = {
            let space = builder.build_alloca(int_ty, "space");
            let float_ptr = builder.build_pointer_cast(space, float_ptr_ty, "float_ptr");
            builder.build_store(float_ptr, value);
            builder.build_load(space, "float_bits").into_int_value()
        };

        let (shift_amount, exponent_mask, invalid_exponent) = match float_size {
            FloatSize::Bits32 => (23, 0b01111111100000000000000000000000, 0b11111111),
            FloatSize::Bits64 => (
                52,
                0b0111111111110000000000000000000000000000000000000000000000000000,
                0b11111111111,
            ),
        };

        builder.build_and(
            float_bits,
            int_ty.const_int(exponent_mask, false),
            "masked_bits",
        );

        (
            builder.build_right_shift(
                float_bits,
                int_ty.const_int(shift_amount, false),
                false,
                "exponent",
            ),
            invalid_exponent,
        )
    };

    let is_invalid_float = builder.build_or(
        builder.build_int_compare(
            IntPredicate::EQ,
            exponent,
            int_ty.const_int(invalid_exponent, false),
            "is_not_normal",
        ),
        builder.build_or(
            builder.build_float_compare(
                FloatPredicate::ULT,
                value,
                float_ty.const_float(lower_bounds),
                "less_than_lower_bounds",
            ),
            builder.build_float_compare(
                FloatPredicate::UGT,
                value,
                float_ty.const_float(upper_bound),
                "greater_than_upper_bounds",
            ),
            "float_not_in_bounds",
        ),
        "is_invalid_float",
    );

    let is_invalid_float = builder
        .build_call(
            intrinsics.expect_i1,
            &[
                is_invalid_float.as_basic_value_enum(),
                intrinsics.i1_ty.const_int(0, false).as_basic_value_enum(),
            ],
            "is_invalid_float_expect",
        )
        .try_as_basic_value()
        .left()
        .unwrap()
        .into_int_value();

    builder.build_conditional_branch(is_invalid_float, &failure_block, &continue_block);
    builder.position_at_end(&failure_block);
    builder.build_call(
        intrinsics.throw_trap,
        &[intrinsics.trap_illegal_arithmetic],
        "throw",
    );
    builder.build_unreachable();
    builder.position_at_end(&continue_block);
}

fn trap_if_zero_or_overflow(
    builder: &Builder,
    intrinsics: &Intrinsics,
    context: &Context,
    function: &FunctionValue,
    left: IntValue,
    right: IntValue,
) {
    let int_type = left.get_type();

    let (min_value, neg_one_value) = if int_type == intrinsics.i32_ty {
        let min_value = int_type.const_int(i32::min_value() as u64, false);
        let neg_one_value = int_type.const_int(-1i32 as u32 as u64, false);
        (min_value, neg_one_value)
    } else if int_type == intrinsics.i64_ty {
        let min_value = int_type.const_int(i64::min_value() as u64, false);
        let neg_one_value = int_type.const_int(-1i64 as u64, false);
        (min_value, neg_one_value)
    } else {
        unreachable!()
    };

    let should_trap = builder.build_or(
        builder.build_int_compare(
            IntPredicate::EQ,
            right,
            int_type.const_int(0, false),
            "divisor_is_zero",
        ),
        builder.build_and(
            builder.build_int_compare(IntPredicate::EQ, left, min_value, "left_is_min"),
            builder.build_int_compare(IntPredicate::EQ, right, neg_one_value, "right_is_neg_one"),
            "div_will_overflow",
        ),
        "div_should_trap",
    );

    let should_trap = builder
        .build_call(
            intrinsics.expect_i1,
            &[
                should_trap.as_basic_value_enum(),
                intrinsics.i1_ty.const_int(0, false).as_basic_value_enum(),
            ],
            "should_trap_expect",
        )
        .try_as_basic_value()
        .left()
        .unwrap()
        .into_int_value();

    let shouldnt_trap_block = context.append_basic_block(function, "shouldnt_trap_block");
    let should_trap_block = context.append_basic_block(function, "should_trap_block");
    builder.build_conditional_branch(should_trap, &should_trap_block, &shouldnt_trap_block);
    builder.position_at_end(&should_trap_block);
    builder.build_call(
        intrinsics.throw_trap,
        &[intrinsics.trap_illegal_arithmetic],
        "throw",
    );
    builder.build_unreachable();
    builder.position_at_end(&shouldnt_trap_block);
}

fn trap_if_zero(
    builder: &Builder,
    intrinsics: &Intrinsics,
    context: &Context,
    function: &FunctionValue,
    value: IntValue,
) {
    let int_type = value.get_type();
    let should_trap = builder.build_int_compare(
        IntPredicate::EQ,
        value,
        int_type.const_int(0, false),
        "divisor_is_zero",
    );

    let should_trap = builder
        .build_call(
            intrinsics.expect_i1,
            &[
                should_trap.as_basic_value_enum(),
                intrinsics.i1_ty.const_int(0, false).as_basic_value_enum(),
            ],
            "should_trap_expect",
        )
        .try_as_basic_value()
        .left()
        .unwrap()
        .into_int_value();

    let shouldnt_trap_block = context.append_basic_block(function, "shouldnt_trap_block");
    let should_trap_block = context.append_basic_block(function, "should_trap_block");
    builder.build_conditional_branch(should_trap, &should_trap_block, &shouldnt_trap_block);
    builder.position_at_end(&should_trap_block);
    builder.build_call(
        intrinsics.throw_trap,
        &[intrinsics.trap_illegal_arithmetic],
        "throw",
    );
    builder.build_unreachable();
    builder.position_at_end(&shouldnt_trap_block);
}

fn resolve_memory_ptr(
    builder: &Builder,
    intrinsics: &Intrinsics,
    context: &Context,
    function: &FunctionValue,
    state: &mut State,
    ctx: &mut CtxType,
    memarg: MemoryImmediate,
    ptr_ty: PointerType,
) -> Result<PointerValue, BinaryReaderError> {
    // Ignore alignment hint for the time being.
    let imm_offset = intrinsics.i64_ty.const_int(memarg.offset as u64, false);
    let var_offset_i32 = state.pop1()?.into_int_value();
    let var_offset =
        builder.build_int_z_extend(var_offset_i32, intrinsics.i64_ty, &state.var_name());
    let effective_offset = builder.build_int_add(var_offset, imm_offset, &state.var_name());
    let memory_cache = ctx.memory(MemoryIndex::new(0));

    let mem_base_int = match memory_cache {
        MemoryCache::Dynamic {
            ptr_to_base_ptr,
            ptr_to_bounds,
        } => {
            let base = builder
                .build_load(ptr_to_base_ptr, "base")
                .into_pointer_value();
            let bounds = builder.build_load(ptr_to_bounds, "bounds").into_int_value();

            let base_as_int = builder.build_ptr_to_int(base, intrinsics.i64_ty, "base_as_int");

            let base_in_bounds = builder.build_int_compare(
                IntPredicate::ULT,
                effective_offset,
                bounds,
                "base_in_bounds",
            );

            let base_in_bounds = builder
                .build_call(
                    intrinsics.expect_i1,
                    &[
                        base_in_bounds.as_basic_value_enum(),
                        intrinsics.i1_ty.const_int(1, false).as_basic_value_enum(),
                    ],
                    "base_in_bounds_expect",
                )
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();

            let in_bounds_continue_block =
                context.append_basic_block(function, "in_bounds_continue_block");
            let not_in_bounds_block = context.append_basic_block(function, "not_in_bounds_block");
            builder.build_conditional_branch(
                base_in_bounds,
                &in_bounds_continue_block,
                &not_in_bounds_block,
            );
            builder.position_at_end(&not_in_bounds_block);
            builder.build_call(
                intrinsics.throw_trap,
                &[intrinsics.trap_memory_oob],
                "throw",
            );
            builder.build_unreachable();
            builder.position_at_end(&in_bounds_continue_block);

            base_as_int
        }
        MemoryCache::Static {
            base_ptr,
            bounds: _,
        } => builder.build_ptr_to_int(base_ptr, intrinsics.i64_ty, "base_as_int"),
    };

    let effective_address_int =
        builder.build_int_add(mem_base_int, effective_offset, &state.var_name());
    Ok(builder.build_int_to_ptr(effective_address_int, ptr_ty, &state.var_name()))
}
