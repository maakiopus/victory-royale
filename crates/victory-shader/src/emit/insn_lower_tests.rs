//! Lowering tests for single machine instructions: each builds a synthetic
//! one-thread compute program around the instructions under test and asks
//! the translator for a module, so an instruction's lowering can be checked
//! without a GPU or a driver.
//!
//! They sit here rather than at the foot of `lower.rs` because that file is
//! within a few kilobytes of the asset policy's 512 KiB per-file limit
//! (`cargo xtask policy`), as `valu_tests` and `waterfall_tests` already do
//! for the tests that need a device.

use crate::native::{
    ComputeIo, HwStage, NativeProgram, ResourceType, RootParam, RootType, StageIo, UsedResource,
};
use crate::{ShaderError, io, ir, resource, structure};

use super::lower::{Lowered, Parts, lower};
use super::lower_tests::{lower_with, root, texture_tables};

const S_ENDPGM: u32 = 0xBF81_0000;

/// Lowers a one-thread compute program that copies four root constants
/// (s0..s3) into v1..v4, runs `body`, and stores v0 to a raw buffer from a
/// descriptor table, so the vector instructions in `body` have an effect.
fn lowered_valu(body: &[u32]) -> Result<Lowered, ShaderError> {
    let mut words = vec![
        0xBE85_0380, // s_mov_b32 s5, 0
        0xF408_0202, // s_load_dwordx4 s[8:11], s[4:5], 0
        0xFA00_0000,
    ];
    // v_mov_b32 v1..v4, s0..s3
    words.extend((0..4u32).map(|i| 0x7E00_0200 | (i + 1) << 17 | i));
    words.extend_from_slice(body);
    words.extend_from_slice(&[
        0xE070_0000, // buffer_store_dword v0, off, s[8:11], 0
        0x8002_0000,
        S_ENDPGM,
    ]);
    let layout = vec![
        RootParam {
            root: RootType::Constants,
            table_offset_dwords: 0,
            sgprs: 4,
            base_register: 0,
            user_data: 0,
            slot: 0,
        },
        RootParam {
            root: RootType::Table,
            table_offset_dwords: 0,
            sgprs: 1,
            base_register: 0,
            user_data: 4,
            slot: 1,
        },
    ];
    let used = vec![UsedResource {
        kind: ResourceType::Uav,
        root: RootType::Table,
        table_offset_dwords: 0,
        lower_bound: 0,
        stride: 0,
        dwords: 4,
        user_data: 4,
        slot: 1,
    }];
    let sgprs: u32 = layout.iter().map(|p| p.sgprs).sum();
    let native = NativeProgram {
        stage: HwStage::Cs,
        wave64: false,
        vgprs: 256,
        layout,
        used,
        io: StageIo::Cs(ComputeIo {
            // The layout's user SGPRs, and 512 bytes of LDS.
            rsrc2: 1 << 15 | sgprs << 1,
            threads: [1, 1, 1],
        }),
        code: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
    };
    let p = ir::build(&native.code, false, 256).unwrap();
    let st = structure::structure(&p).unwrap();
    let res = resource::track(&p, &native.layout, native.stage, Some(&st))?;
    let io = io::raise(&p, &native).unwrap();
    lower(&Parts {
        native: &native,
        p: &p,
        st: &st,
        io: &io,
        res: &res,
        inputs: vec![],
        outputs: vec![],
        prims: vec![],
        mesh_stride: 0,
    })
}

/// Whether [`lowered_valu`] translates `body`.
fn lowers_valu(body: &[u32]) -> Result<(), ShaderError> {
    lowered_valu(body).map(|_| ())
}

#[test]
fn the_24_bit_multiplies_lower_both_halves() {
    // VOP2 `op v0, v1, v2`: v_mul_i32_i24 and v_mul_u32_u24 (the low
    // dword) and v_mul_hi_i32_i24 and v_mul_hi_u32_u24 (the high one).
    for op in [0x09u32, 0x0A, 0x0B, 0x0C] {
        let word = op << 25 | 2 << 9 | 0x101;
        lowers_valu(&[word]).unwrap_or_else(|e| panic!("VOP2 {op:#x}: {e}"));
    }
}

#[test]
fn sums_of_absolute_differences_lower() {
    // VOP3 `op v0, v1, v2, v3`: v_sad_u8, v_sad_hi_u8, v_sad_u16,
    // v_sad_u32 and v_msad_u8.
    for op in [0x15Au32, 0x15B, 0x15C, 0x15D, 0x171] {
        let words = [0xD400_0000 | op << 16, 0x101 | 0x102 << 9 | 0x103 << 18];
        lowers_valu(&words).unwrap_or_else(|e| panic!("VOP3 {op:#x}: {e}"));
    }
}

#[test]
fn a_d16_typed_buffer_load_packs_its_channels() {
    // MTBUF format:27 (16_16_UINT) into v0, from s[8:11]:
    // tbuffer_load_format_xy (opcode 1) and tbuffer_load_format_d16_xy
    // (opcode 9, the high opcode bit in word 1). The d16 form puts both
    // channels in v0, so it emits a different module; taken as the 32-bit
    // form it emitted the same one, with channel x alone in v0 as a dword
    // and channel y in v1.
    let load = |d16: u32| {
        lowered_valu(&[0xE8D9_0000, 0x8002_0000 | d16])
            .unwrap_or_else(|e| panic!("d16 bit {d16:#x}: {e}"))
            .dxil
    };
    let (plain, d16) = (load(0), load(0x0020_0000));
    assert_eq!(plain, load(0), "lowering the same program twice differed");
    assert_ne!(plain, d16, "the d16 load packed no channels");
}

#[test]
fn v_cndmask_takes_a_uniform_or_constant_lane_mask() {
    // A mask structurisation cannot derive from EXEC or a VALU result:
    // `s_cselect_b32 s0, -1, 0`, which is every lane or none by its SCC,
    // and the inline constants -1 (every lane) and 0 (none). Without them
    // the uniform select fell through to the numeric path, which has no
    // console lane numbers here, and the constants were refused outright.
    let body = [
        0xBF06_8000u32, // s_cmp_eq_u32 s0, 0
        0x8500_80C1,    // s_cselect_b32 s0, -1, 0
        0xD501_0000,
        0x0002_0280, // v_cndmask_b32 v0, 0, v1, s0
        0xD501_0000,
        0x0306_0080, // v_cndmask_b32 v0, 0, v0, -1
        0xD501_0000,
        0x0202_0080, // v_cndmask_b32 v0, 0, v0, 0
    ];
    lowers_valu(&body).unwrap();
}

#[test]
fn a_uniform_branch_can_choose_between_immediate_samplers() {
    // A root constant selects wrap or clamp. Keep the sample live with an LDS store.
    let words = [
        0xBE84_0300,
        0xBE85_0380, // s[4:5] = texture table
        0xF40C_0302,
        0xFA00_0000, // s_load_dwordx8 s[12:19], s[4:5], 0
        0xBE89_0380,
        0xBE8A_0380,
        0xBE8B_0380, // sampler words 1-3
        0xBF06_8001, // s_cmp_eq_u32 s1, 0
        0xBF85_0002, // branch to clamp
        0xBE88_0380,
        0xBF82_0002, // wrap, skip clamp
        0xBE88_03FF,
        0x0000_0092, // clamp all axes
        0xF09C_0F08,
        0x0043_0C02, // image_sample_lz v[12:15], ..., s[8:11]
        0xD834_0000,
        0x0000_0C00, // ds_write_b32 v0, v12
        S_ENDPGM,
    ];
    let (mut layout, mut used) = texture_tables();
    layout[1] = root(RootType::Constants, 1, 1);
    used.truncate(1);
    let shader = lower_with(&words, layout, used).unwrap();
    assert_eq!(shader.samplers.len(), 2);
    assert!(shader.samplers.contains(&[0, 0, 0, 0]));
    assert!(shader.samplers.contains(&[0x92, 0, 0, 0]));
}
