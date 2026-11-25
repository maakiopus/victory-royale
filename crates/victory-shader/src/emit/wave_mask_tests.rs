//! Lowering tests for lane masks and first-lane reads across the console's
//! waves: a mask a scalar operation builds over other masks (so no ballot
//! where it is written gives it), a mask whose writer has no number at all,
//! and a pixel program's `v_readfirstlane_b32`, which must serve the host's
//! helper lanes even though they take no part in its wave operations.
//!
//! The programs are synthetic GFX10 words built here; none touches the
//! environment or needs a GPU.

use super::lower::{Lowered, MASK_ELSEWHERE, Parts, lower};
use crate::native::{ComputeIo, HwStage, NativeProgram, StageIo};
use crate::{io, ir, resource, structure};

const S_ENDPGM: u32 = 0xBF81_0000;

/// Lowers a synthetic wave32 compute program with 512 bytes of LDS, no root
/// parameters and two user SGPRs.
fn lower_compute(words: &[u32]) -> Result<Lowered, crate::ShaderError> {
    let code: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let native = NativeProgram {
        stage: HwStage::Cs,
        wave64: false,
        vgprs: 256,
        layout: vec![],
        used: vec![],
        io: StageIo::Cs(ComputeIo {
            rsrc2: 1 << 15 | 2 << 1,
            threads: [1, 1, 1],
        }),
        code,
    };
    lowered(&native)
}

/// Lowers a synthetic wave32 pixel program with no interpolated inputs.
fn lower_pixel(words: &[u32]) -> Result<Lowered, crate::ShaderError> {
    let code: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let native = NativeProgram {
        stage: HwStage::Ps,
        wave64: false,
        vgprs: 256,
        layout: vec![],
        used: vec![],
        io: StageIo::Ps {
            inputs: vec![],
            input_addr: 0,
            db_shader_control: 0,
            patches: vec![],
        },
        code,
    };
    lowered(&native)
}

fn lowered(native: &NativeProgram) -> Result<Lowered, crate::ShaderError> {
    let p = ir::build(&native.code, false, 256).unwrap();
    let st = structure::structure(&p).unwrap();
    let res = resource::track(&p, &native.layout, native.stage, Some(&st))?;
    let io = io::raise(&p, native).unwrap();
    lower(&Parts {
        native,
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

/// A helper lane has no part in the host's wave operations, so a
/// `WaveReadLaneFirst` left to helper lanes alone reads nothing and a
/// waterfall loop never ends for them: a pixel program's first-lane read
/// gives a helper lane its own value. Compute has no helper lanes and asks
/// nothing of `isHelperLane`.
#[test]
fn a_pixel_first_lane_read_gives_a_helper_lane_its_own_value() {
    let read_first_lane = [
        0x7E00_0500, // v_readfirstlane_b32 s0, v0
        0x7E04_0200, // v_mov_b32 v2, s0
    ];
    let pixel: Vec<u32> = read_first_lane
        .iter()
        .copied()
        .chain([
            0xF800_180F,
            0x0202_0202, // exp mrt0, v2, v2, v2, v2 done vm
            S_ENDPGM,
        ])
        .collect();
    let l = lower_pixel(&pixel).expect("the pixel program lowers");
    assert!(l.wave_ops);
    assert!(l.helper_lane_own, "a pixel first-lane read asks for it");

    // The same read in compute, with the result kept live in LDS.
    let compute: Vec<u32> = read_first_lane
        .iter()
        .copied()
        .chain([
            0xD834_0000,
            0x0000_0200, // ds_write_b32 v0, v2
            S_ENDPGM,
        ])
        .collect();
    let l = lower_compute(&compute).expect("the compute program lowers");
    assert!(l.wave_ops);
    assert!(!l.helper_lane_own, "compute has no helper lanes");
}

/// A mask a scalar operation builds over other masks can hold lanes that are
/// not in EXEC where it is written, so no ballot there gives it. With console
/// lanes it is that operation over its operands' numbers.
#[test]
fn a_mask_combined_by_a_scalar_operation_is_read_as_that_operation() {
    // v_cmp_eq_u32 vcc, 0, v0; narrow EXEC to those lanes, then build a mask
    // of the lanes that are NOT there and store it.
    let masked = |combine: &[u32]| -> Vec<u32> {
        [
            0x7D84_0080, // v_cmp_eq_u32 vcc, 0, v0
            0xBEFE_036A, // s_mov_b32 exec_lo, vcc_lo
        ]
        .iter()
        .chain(combine)
        .copied()
        .chain([
            0x7E02_020A, // v_mov_b32 v1, s10
            0xD834_0000,
            0x0000_0100, // ds_write_b32 v0, v1
            S_ENDPGM,
        ])
        .collect()
    };
    // Each of these leaves s10 set only in lanes outside EXEC.
    let combines: [&[u32]; 4] = [
        &[0xBE8A_076A],              // s_not_b32 s10, vcc_lo
        &[0x8C0A_6A6A],              // s_nand_b32 s10, vcc_lo, vcc_lo
        &[0xBE8A_076A, 0xBE8A_030A], // s_not_b32 s10, vcc_lo; s_mov_b32 s10, s10
        &[
            0xBE8B_076A, // s_not_b32 s11, vcc_lo
            0xBE8C_0381, // s_mov_b32 s12, 1
            0xBF07_800C, // s_cmp_lg_u32 s12, 0
            0x850A_6A0B, // s_cselect_b32 s10, s11, vcc_lo
        ],
    ];
    for combine in combines {
        let words = masked(combine);
        lower_compute(&words).unwrap_or_else(|e| panic!("{combine:x?}: {e}"));
    }
}

/// A mask whose own writer the lowering cannot turn into a number is still
/// refused, with the reason testers group by.
#[test]
fn a_mask_held_by_an_opaque_writer_is_still_refused_as_data() {
    // s_wqm_b32 brings in each lane's quad: no number over the operands.
    let words = [
        0x7D84_0080, // v_cmp_eq_u32 vcc, 0, v0
        0xBEFE_036A, // s_mov_b32 exec_lo, vcc_lo
        0xBE8A_096A, // s_wqm_b32 s10, vcc_lo
        0x7E02_020A, // v_mov_b32 v1, s10
        0xD834_0000,
        0x0000_0100, // ds_write_b32 v0, v1
        S_ENDPGM,
    ];
    let Err(err) = lower_compute(&words) else {
        panic!("a quad mask read as data must be refused");
    };
    assert!(
        matches!(
            err,
            crate::ShaderError::Unsupported {
                what: MASK_ELSEWHERE,
                ..
            }
        ),
        "{err}"
    );
}
