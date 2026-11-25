//! Descriptors the code reads only as numbers (`Lower::descriptor_data`):
//! no instruction reads through them, so their kind comes from the
//! used-resource entry (a buffer in a table) or from how the code decodes
//! them (an image from the view heap sized for another one's coordinates).
//! The lowering checks run everywhere; the GPU checks (host GPU and WARP)
//! store the dwords as rebuilt and compare them with the views' sizes.

use super::lower_tests::{lower_with, root};
use crate::native::{ResourceType, RootParam, RootType, UsedResource};

const S_ENDPGM: u32 = 0xBF81_0000;
/// `s_waitcnt lgkmcnt(0)`.
const WAIT: u32 = 0xBF8C_C07F;
/// `s_mov_b32 s1, 0`: the high half of the address in s[0:1].
const HIGH_ZERO: u32 = 0xBE81_0380;

/// SMEM `s_load_dword{x4,x8}` (`op` 2 or 3) of `s[dst..]` from `s[0:1]` plus `offset`.
const fn s_load(op: u32, dst: u32, offset: u32) -> [u32; 2] {
    [0xF400_0000 | op << 18 | dst << 6, 125 << 25 | offset]
}

/// VOP1 `v_mov_b32 v<v>, s<s>`.
const fn v_from_s(v: u32, s: u32) -> u32 {
    0x7E00_0200 | v << 17 | s
}

/// MUBUF `buffer_store_dword v<v>, off, s[4:7], 0 offset:<4 * dword>`.
const fn store(v: u32, dword: u32) -> [u32; 2] {
    [0xE070_0000 | (4 * dword), 0x8001_0000 | v << 8]
}

/// A table of a buffer SRV of 16-byte records (s[0:1]) and a raw UAV
/// after it. The SRV's descriptor goes to s[8:11]; only its record count
/// (dword 2) is stored, as a program does to skip an empty buffer.
fn record_count_program() -> (Vec<u32>, Vec<RootParam>, Vec<UsedResource>) {
    let mut words = vec![HIGH_ZERO];
    words.extend(s_load(2, 8, 0));
    words.extend(s_load(2, 4, 0x20));
    words.push(WAIT);
    words.push(v_from_s(0, 10));
    words.extend(store(0, 0));
    words.push(S_ENDPGM);
    let entry = |kind, at, stride| UsedResource {
        kind,
        root: RootType::Table,
        table_offset_dwords: at,
        lower_bound: 0,
        stride,
        dwords: 4,
        user_data: 0,
        slot: 0,
    };
    (
        words,
        vec![root(RootType::Table, 0, 0)],
        vec![
            entry(ResourceType::Srv, 0, 16),
            entry(ResourceType::Uav, 8, 0),
        ],
    )
}

/// From the view heap (s[0:1]): an image descriptor to s[20:27] that the
/// code decodes as the title's pixel shaders do (width and height shifted
/// by the base level), and a raw UAV (heap index 1) to s[4:7]. Stored:
/// width, height, base level, last level and type; with `depth` also dword
/// 4 (depth or layers - 1).
fn image_size_program(depth: bool) -> (Vec<u32>, Vec<RootParam>) {
    let mut words = vec![HIGH_ZERO];
    words.extend(s_load(3, 20, 0));
    words.extend(s_load(2, 4, 0x20));
    words.push(WAIT);
    words.extend([
        0x8F10_8216, // s_lshl_b32 s16, s22, 2
        0x900C_9E15, // s_lshr_b32 s12, s21, 30
        0x938F_FF16,
        0x0010_000E, // s_bfe_u32 s15, s22, 14..29 (height - 1)
        0x8810_100C, // s_or_b32 s16, s12, s16
        0x938C_FF17,
        0x0004_000C, // s_bfe_u32 s12, s23, 12..15 (base level)
        0x8710_FF10,
        0x0000_FFFF, // s_and_b32 s16, s16, 0xffff (width - 1)
        0x9391_FF17,
        0x0004_0010, // s_bfe_u32 s17, s23, 16..19 (last level)
        0x9392_FF17,
        0x0004_001C, // s_bfe_u32 s18, s23, 28..31 (type)
        0x810F_810F, // s_add_i32 s15, s15, 1
        0x8110_8110, // s_add_i32 s16, s16, 1
        0x900F_0C0F, // s_lshr_b32 s15, s15, s12
        0x900D_0C10, // s_lshr_b32 s13, s16, s12
    ]);
    for (v, s) in [(0, 13), (1, 15), (2, 12), (3, 17), (4, 18)] {
        words.push(v_from_s(v, s));
        words.extend(store(v, v));
    }
    if depth {
        words.push(v_from_s(5, 24));
        words.extend(store(5, 5));
    }
    words.push(S_ENDPGM);
    (words, vec![root(RootType::ResourceHeap, 0, 0)])
}

#[test]
fn a_buffers_record_count_read_alone_lowers() {
    let (words, layout, used) = record_count_program();
    lower_with(&words, layout, used).unwrap();
}

#[test]
fn a_heap_images_size_read_alone_lowers_as_2d() {
    let (words, layout) = image_size_program(false);
    lower_with(&words, layout, Vec::new()).unwrap();
}

#[test]
fn a_heap_images_depth_read_alone_stays_unknown() {
    // Dword 4 tells a 2D texture from a 3D one or an array: no guess.
    let (words, layout) = image_size_program(true);
    let Err(e) = lower_with(&words, layout, Vec::new()) else {
        panic!("a descriptor whose depth is read lowered as a 2D texture");
    };
    assert!(
        e.to_string().contains("a descriptor read only as data"),
        "{e}"
    );
}

#[cfg(windows)]
mod gpu {
    use super::super::host::{Dispatch, Host, Root, Stride, View};
    use super::super::intersect_tests::build_compute_shader;
    use super::*;
    use crate::native::{ComputeIo, HwStage, NativeProgram, StageIo};

    /// Each device's first UAV after running `words`.
    fn run(
        words: Vec<u32>,
        layout: Vec<RootParam>,
        used: Vec<UsedResource>,
        views: &[View],
    ) -> Vec<(&'static str, Vec<u32>)> {
        let native = NativeProgram {
            stage: HwStage::Cs,
            wave64: false,
            vgprs: 16,
            layout,
            used,
            io: StageIo::Cs(ComputeIo {
                rsrc2: 2 << 1,
                threads: [1, 1, 1],
            }),
            code: words.into_iter().flat_map(u32::to_le_bytes).collect(),
        };
        let (shader, _) = build_compute_shader(&native).unwrap();
        let mut out = Vec::new();
        for (name, warp) in [("WARP", true), ("hardware", false)] {
            let Ok(host) = Host::new(warp) else {
                eprintln!("no SM 6.6 {name} device; skipped");
                continue;
            };
            let (root, pso) = host.compute_pipeline(&shader).unwrap();
            let uavs = host
                .dispatch(
                    &root,
                    &pso,
                    &Dispatch {
                        roots: vec![(
                            0,
                            Root::Constants(vec![0; shader.user_data_dwords() as usize]),
                        )],
                        views,
                        samplers: &[],
                        groups: [1, 1, 1],
                    },
                )
                .unwrap();
            out.push((name, uavs[0].clone()));
        }
        out
    }

    #[test]
    fn a_buffers_record_count_is_its_size_in_records() {
        let (words, layout, used) = record_count_program();
        // 48 dwords: 12 records of 16 bytes.
        let data = [0u32; 48];
        let views = [
            View::Buffer {
                data: &data,
                stride: Stride::Raw,
            },
            View::RawUav(1),
        ];
        for (name, got) in run(words, layout, used, &views) {
            assert_eq!(got, [12], "{name}");
        }
    }

    #[test]
    fn a_heap_images_size_decodes_to_the_views() {
        let (words, layout) = image_size_program(false);
        let (w, h) = (37, 19);
        let texels = vec![[0.0f32; 4]; (w * h) as usize];
        let views = [View::Texture(w, h, &texels), View::RawUav(5)];
        for (name, got) in run(words, layout, Vec::new(), &views) {
            // Width and height at the base level (0), one level, a 2D image (type 9).
            assert_eq!(got, [w, h, 0, 0, 9], "{name}");
        }
    }
}
