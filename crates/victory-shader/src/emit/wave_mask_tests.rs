//! Lowering tests for lane masks and first-lane reads across the console's
//! waves: a pixel program's `v_readfirstlane_b32` must serve the host's
//! helper lanes even though they take no part in its wave operations.
//!
//! The programs are synthetic GFX10 words built here; none touches the
//! environment or needs a GPU.

use super::lower::{Lowered, Parts, lower};
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
