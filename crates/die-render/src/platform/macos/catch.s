// `die_objc_try`, as catch.rs has it in Objective-C, as clang writes it. Not Rust because Rust
// has no landing pad an Objective-C personality can use, and not C because of D16.

    .private_extern _die_objc_try
    .p2align 2
_die_objc_try:
Ldie_try_begin:
    .cfi_startproc
    .cfi_personality 155, ___objc_personality_v0
    .cfi_lsda 16, Ldie_try_lsda
    stp x20, x19, [sp, #-32]!
    stp x29, x30, [sp, #16]
    add x29, sp, #16
    .cfi_def_cfa w29, 16
    .cfi_offset w30, -8
    .cfi_offset w29, -16
    .cfi_offset w19, -24
    .cfi_offset w20, -32
    mov x19, x2
    mov x8, x0
Ldie_try_call:
    mov x0, x1
    blr x8
Ldie_try_called:
    mov w0, #0
Ldie_try_return:
    ldp x29, x30, [sp, #16]
    ldp x20, x19, [sp], #32
    ret
Ldie_try_caught:
    mov x20, x0
    cmp w1, #1
    b.ne Ldie_try_resume
    mov x0, x20
    bl _objc_begin_catch
Ldie_try_retain:
    bl _objc_retain
Ldie_try_retained:
    str x0, [x19]
    bl _objc_end_catch
    mov w0, #1
    b Ldie_try_return
Ldie_try_cleanup:
    mov x20, x0
    bl _objc_end_catch
Ldie_try_resume:
    mov x0, x20
    bl __Unwind_Resume
Ldie_try_end:
    .cfi_endproc

// The exception table: what the personality reads, with the `@catch (id)` as the only type.
    .section __TEXT,__gcc_except_tab
    .p2align 2, 0x0
Ldie_try_lsda:
    .byte 255
    .byte 155
    .uleb128 Ldie_try_ttbase-Ldie_try_ttref
Ldie_try_ttref:
    .byte 1
    .uleb128 Ldie_try_cs_end-Ldie_try_cs_begin
Ldie_try_cs_begin:
    .uleb128 Ldie_try_call-Ldie_try_begin
    .uleb128 Ldie_try_called-Ldie_try_call
    .uleb128 Ldie_try_caught-Ldie_try_begin
    .byte 1
    .uleb128 Ldie_try_called-Ldie_try_begin
    .uleb128 Ldie_try_retain-Ldie_try_called
    .byte 0
    .byte 0
    .uleb128 Ldie_try_retain-Ldie_try_begin
    .uleb128 Ldie_try_retained-Ldie_try_retain
    .uleb128 Ldie_try_cleanup-Ldie_try_begin
    .byte 0
    .uleb128 Ldie_try_retained-Ldie_try_begin
    .uleb128 Ldie_try_end-Ldie_try_retained
    .byte 0
    .byte 0
Ldie_try_cs_end:
    .byte 1
    .byte 0
    .p2align 2, 0x0
Ldie_try_type:
    .long _OBJC_EHTYPE_id@GOT-Ldie_try_type
Ldie_try_ttbase:
    .text
