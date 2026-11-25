//! Indirect draws run from the title's own arguments when the CPU has
//! checked them (`VICTORY_INDIRECT_RAW=3`; off by default).
//!
//! The console runs an indirect draw straight from the record the title
//! wrote. The host cannot trust an arbitrary record (a garbage count hangs
//! or resets the card), so `crate::draw` clamps every record in a compute
//! pass of its own before `ExecuteIndirect` (`crate::indirect::bound`).
//! For the title's thousands of single draws a frame, that pass and the
//! barriers around it switch the card between compute and drawing for
//! every record.
//!
//! With the switch, a draw or indexed draw of one record and no count
//! buffer is drawn from the title's own record when all of these hold:
//!
//! - nothing of the card's (a shader store, a buffer copy, a bottom-of-pipe
//!   write, a texture read back) was declared to write the record's bytes
//!   in this frame or the two before ([`WriteLog`]): such a record may hold
//!   at the draw what the CPU cannot see now;
//! - the CPU can read the record now;
//! - it asks for something that can run ([`sane`]): counts up to 2^24,
//!   their product up to 2^26, starts below 2^28.
//!
//! Every other indirect call is clamped as before, and with the switch
//! unset or set to anything else nothing here runs.
//!
//! shortcut: the record is read when the draw is recorded, and the host
//! copy is brought up to date from title memory when the list is
//! submitted. A record the title's CPU changes in between is drawn
//! unchecked; and a card write to the record that is not declared to
//! `crate::shadow::Shadow::gpu_writes` (nor copied back) is not seen. The
//! buffers the starts index are not looked at either.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Draw and indexed draw (PC argument numbering).
pub(crate) const DRAW: u32 = 0;
pub(crate) const INDEXED: u32 = 1;

/// The most instances or vertices (indices) a checked record asks for, the
/// most of their product, and the first start it may not reach.
const CAP: u32 = 1 << 24;
const PRODUCT: u64 = 1 << 26;
const START: u32 = 1 << 28;

/// How many frames before the current one a card write keeps a record from
/// being trusted: the flights a submission can still be in.
const KEPT_FRAMES: u64 = 2;

/// The most separate written spans kept; past it everything counts as
/// written until the spans would have aged out.
const MOST_SPANS: usize = 1 << 14;

/// Whether `VICTORY_INDIRECT_RAW=3` is set.
#[cfg(windows)]
pub(crate) fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| checked(std::env::var_os("VICTORY_INDIRECT_RAW").as_deref()))
}

/// Whether a value of `VICTORY_INDIRECT_RAW` asks for checked records.
fn checked(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|v| v == "3")
}

/// What became of a single draw the switch looked at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Drawn as the title wrote it: the CPU checked the record.
    Raw = 0,
    /// Clamped: the card may write the record.
    Written = 1,
    /// Clamped: the record failed the check.
    Bad = 2,
    /// Clamped: the record could not be read.
    Unread = 3,
}

/// One indirect call as the title made it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Call {
    /// PC argument numbering.
    pub(crate) kind: u32,
    pub(crate) stride: u32,
    pub(crate) max: u32,
    /// Byte of the record in its host copy.
    pub(crate) offset: u64,
    /// Whether the title gave a count buffer.
    pub(crate) counted: bool,
}

/// The bytes of the record the switch would check, or `None` if the call
/// is not one it looks at (it is clamped without being counted).
pub(crate) fn record_len(call: Call) -> Option<usize> {
    let len = match call.kind {
        DRAW => 16,
        INDEXED => 20,
        _ => return None,
    };
    let single = call.max == 1 && !call.counted;
    // The host reads arguments at 4-byte offsets only, and a stride shorter
    // than the record names no valid signature.
    let laid_out = call.offset.is_multiple_of(4) && call.stride as usize >= len;
    (single && laid_out).then_some(len)
}

/// Whether a draw (`kind` [`DRAW`], 16 bytes) or indexed draw
/// ([`INDEXED`], 20 bytes) record asks for something that can run.
pub(crate) fn sane(kind: u32, record: &[u8]) -> bool {
    let word = |i: usize| {
        record
            .get(i * 4..i * 4 + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let words = if kind == INDEXED { 5 } else { 4 };
    let Some(w) = (0..words).map(word).collect::<Option<Vec<u32>>>() else {
        return false;
    };
    let (count, instances) = (w[0], w[1]);
    // Indexed: first index, base vertex (signed), first instance.
    let starts = if kind == INDEXED {
        w[2].max((w[3] as i32).unsigned_abs()).max(w[4])
    } else {
        w[2].max(w[3])
    };
    count <= CAP
        && instances <= CAP
        && u64::from(count) * u64::from(instances) <= PRODUCT
        && starts < START
}

/// The verdict on an eligible record of `len` bytes: `written` says whether
/// the card may write it, `read` fills the buffer with it (false if it
/// cannot be read). The record is read only if it is not written.
pub(crate) fn judge(
    kind: u32,
    len: usize,
    written: bool,
    read: impl FnOnce(&mut [u8]) -> bool,
) -> Verdict {
    if written {
        return Verdict::Written;
    }
    let mut record = [0u8; 20];
    let Some(record) = record.get_mut(..len) else {
        return Verdict::Bad;
    };
    if !read(record) {
        return Verdict::Unread;
    }
    if sane(kind, record) {
        Verdict::Raw
    } else {
        Verdict::Bad
    }
}

/// Per-verdict counts since the last frame line.
pub(crate) struct Counts([AtomicU64; 4]);

impl Counts {
    pub(crate) const fn new() -> Self {
        Self([const { AtomicU64::new(0) }; 4])
    }

    pub(crate) fn add(&self, verdict: Verdict) {
        self.0[verdict as usize].fetch_add(1, Ordering::Relaxed);
    }

    /// The counts by verdict (raw, written, bad, unread), zeroed.
    pub(crate) fn take(&self) -> [u64; 4] {
        [0, 1, 2, 3].map(|i| self.0[i].swap(0, Ordering::Relaxed))
    }
}

/// Title memory the card was declared to write, by frame: disjoint spans,
/// start → (end, the last frame a write touched them). Touching or
/// overlapping writes merge, keeping the latest frame (a part may be held
/// back a little longer than it was written; never shorter).
pub(crate) struct WriteLog {
    swept: u64,
    spans: BTreeMap<u64, (u64, u64)>,
    /// Too many spans to keep: everything counts as written up to and
    /// including this frame.
    full_until: Option<u64>,
}

impl WriteLog {
    pub(crate) const fn new() -> Self {
        Self {
            swept: 0,
            spans: BTreeMap::new(),
            full_until: None,
        }
    }

    /// Drops what aged out by `frame`.
    fn sweep(&mut self, frame: u64) {
        if self.swept == frame {
            return;
        }
        self.swept = frame;
        self.spans.retain(|_, (_, at)| *at + KEPT_FRAMES >= frame);
        if self.full_until.is_some_and(|until| until < frame) {
            self.full_until = None;
        }
    }

    /// The card writes `[address, address + len)` in `frame`.
    pub(crate) fn note(&mut self, frame: u64, address: u64, len: u64) {
        if len == 0 {
            return;
        }
        self.sweep(frame);
        let (mut start, mut end, mut last) = (address, address.saturating_add(len), frame);
        while let Some((&from, &(to, at))) = self.spans.range(..=end).next_back() {
            if to < start {
                break;
            }
            self.spans.remove(&from);
            start = start.min(from);
            end = end.max(to);
            last = last.max(at);
        }
        self.spans.insert(start, (end, last));
        if self.spans.len() > MOST_SPANS {
            self.spans.clear();
            self.full_until = Some(frame + KEPT_FRAMES);
        }
    }

    /// Whether the card was declared to write any of `[address, address +
    /// len)` in `frame` or the [`KEPT_FRAMES`] before it.
    pub(crate) fn hit(&self, frame: u64, address: u64, len: u64) -> bool {
        if self.full_until.is_some_and(|until| until >= frame) {
            return true;
        }
        let end = address.saturating_add(len);
        // Spans are disjoint: only the last one starting before `end` can
        // reach past `address`.
        self.spans
            .range(..end)
            .next_back()
            .is_some_and(|(_, &(to, at))| to > address && at + KEPT_FRAMES >= frame)
    }
}

#[cfg(windows)]
static LOG: std::sync::Mutex<WriteLog> = std::sync::Mutex::new(WriteLog::new());

#[cfg(windows)]
static COUNTS: Counts = Counts::new();

/// The card writes `[address, address + len)` of title memory (a shader
/// store, a buffer copy, a bottom-of-pipe write, a read back): noted only
/// with the switch on.
#[cfg(windows)]
pub(crate) fn card_writes(address: u64, len: u64) {
    if !on() {
        return;
    }
    let frame = crate::frame::current();
    LOG.lock()
        .unwrap_or_else(|e| e.into_inner())
        .note(frame, address, len);
}

/// Whether an indirect call at title address `address` is drawn from the
/// title's record unclamped; counted when the switch looks at it.
#[cfg(windows)]
pub(crate) fn trust(call: Call, address: u64) -> bool {
    if !on() {
        return false;
    }
    let Some(len) = record_len(call) else {
        return false;
    };
    let frame = crate::frame::current();
    let written = LOG
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .hit(frame, address, len as u64);
    let verdict = judge(call.kind, len, written, |record| {
        crate::host::read_into(address as *const u8, record)
    });
    COUNTS.add(verdict);
    verdict == Verdict::Raw
}

/// The frame-rate line's companion: what the switch did per frame.
#[cfg(windows)]
pub(crate) fn line(frames: f64) {
    let [raw, written, bad, unread] = COUNTS.take().map(|n| n as f64 / frames);
    if raw + written + bad + unread == 0.0 {
        return;
    }
    tracing::info!(
        "VICTORY_INDIRECT_RAW=3 single draws per frame: {raw:.0} drawn as the title wrote them (record checked on the CPU), \
         {written:.0} clamped (the card may write the record), {bad:.0} clamped (record failed the check), \
         {unread:.0} clamped (unreadable)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(words: [u32; 4]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    fn indexed(words: [u32; 5]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    fn single(kind: u32) -> Call {
        Call {
            kind,
            stride: 20,
            max: 1,
            offset: 64,
            counted: false,
        }
    }

    fn from(bytes: Vec<u8>) -> impl FnOnce(&mut [u8]) -> bool {
        move |record| {
            record.copy_from_slice(&bytes[..record.len()]);
            true
        }
    }

    #[test]
    fn a_sane_unwritten_record_is_drawn_raw() {
        let call = single(DRAW);
        let len = record_len(call).expect("a single draw is looked at");
        let record = draw([3, 100, 0, 0]);
        assert_eq!(judge(DRAW, len, false, from(record)), Verdict::Raw);
        let call = single(INDEXED);
        let len = record_len(call).expect("a single indexed draw is looked at");
        let record = indexed([36, 4, 1024, (-8i32) as u32, 2]);
        assert_eq!(judge(INDEXED, len, false, from(record)), Verdict::Raw);
    }

    #[test]
    fn a_record_the_card_may_write_is_clamped_without_reading_it() {
        let verdict = judge(DRAW, 16, true, |_| panic!("a written record is not read"));
        assert_eq!(verdict, Verdict::Written);
    }

    #[test]
    fn a_shader_write_in_the_last_three_frames_marks_the_record() {
        let mut log = WriteLog::new();
        log.note(10, 0x1000, 0x100);
        assert!(log.hit(10, 0x1080, 20));
        assert!(log.hit(12, 0x10f0, 20), "an overlap at the span's end");
        assert!(log.hit(12, 0x0ff0, 20), "an overlap at the span's start");
        assert!(!log.hit(12, 0x1100, 20), "the byte after the span");
        assert!(!log.hit(12, 0x0fec, 20), "the bytes before the span");
        assert!(!log.hit(13, 0x1080, 20), "three frames later");
    }

    #[test]
    fn merged_writes_keep_the_latest_frame() {
        let mut log = WriteLog::new();
        log.note(1, 0x1000, 0x100);
        log.note(5, 0x1100, 0x100);
        log.note(5, 0x0800, 0x900);
        assert!(log.hit(7, 0x1000, 16));
        assert!(log.hit(7, 0x0800, 16));
        assert!(!log.hit(8, 0x1000, 16));
        assert_eq!(log.spans.len(), 1);
    }

    #[test]
    fn too_many_spans_count_as_all_written_until_they_age_out() {
        let mut log = WriteLog::new();
        for i in 0..=MOST_SPANS as u64 {
            log.note(4, i * 64, 16);
        }
        assert!(log.hit(4, 1 << 40, 16));
        assert!(log.hit(6, 1 << 40, 16));
        log.note(7, 0, 4);
        assert!(!log.hit(7, 1 << 40, 16));
    }

    #[test]
    fn an_insane_record_is_clamped() {
        let too_many = [CAP + 1, 1, 0, 0];
        let product = [1 << 14, 1 << 13, 0, 0];
        let far_start = [3, 1, START, 0];
        let all_ones = [u32::MAX; 4];
        for words in [too_many, product, far_start, all_ones] {
            assert_eq!(
                judge(DRAW, 16, false, from(draw(words))),
                Verdict::Bad,
                "{words:?}"
            );
        }
        let far_base = indexed([3, 1, 0, (-(START as i32)) as u32, 0]);
        assert_eq!(judge(INDEXED, 20, false, from(far_base)), Verdict::Bad);
        let far_instance = indexed([3, 1, 0, 0, START]);
        assert_eq!(judge(INDEXED, 20, false, from(far_instance)), Verdict::Bad);
        let at_the_limits = draw([1 << 13, 1 << 13, START - 1, 0]);
        assert_eq!(judge(DRAW, 16, false, from(at_the_limits)), Verdict::Raw);
    }

    #[test]
    fn an_unreadable_record_is_clamped() {
        assert_eq!(judge(INDEXED, 20, false, |_| false), Verdict::Unread);
    }

    #[test]
    fn only_single_uncounted_aligned_draws_are_looked_at() {
        let base = single(DRAW);
        assert_eq!(record_len(base), Some(16));
        assert_eq!(record_len(Call { max: 2, ..base }), None);
        assert_eq!(record_len(Call { max: 0, ..base }), None);
        assert_eq!(
            record_len(Call {
                counted: true,
                ..base
            }),
            None
        );
        assert_eq!(record_len(Call { offset: 66, ..base }), None);
        assert_eq!(record_len(Call { stride: 12, ..base }), None);
        assert_eq!(record_len(Call { kind: 2, ..base }), None, "dispatch");
        assert_eq!(record_len(Call { kind: 10, ..base }), None, "mesh");
        let indexed = single(INDEXED);
        assert_eq!(
            record_len(Call {
                stride: 16,
                ..indexed
            }),
            None
        );
    }

    #[test]
    fn only_the_value_3_turns_the_check_on() {
        assert!(checked(Some("3".as_ref())));
        for value in [None, Some("1"), Some("2"), Some("0"), Some(""), Some("3 ")] {
            assert!(!checked(value.map(AsRef::as_ref)), "{value:?}");
        }
    }

    #[test]
    fn counts_are_taken_by_verdict_and_zeroed() {
        let counts = Counts::new();
        for verdict in [
            Verdict::Raw,
            Verdict::Raw,
            Verdict::Written,
            Verdict::Bad,
            Verdict::Unread,
        ] {
            counts.add(verdict);
        }
        assert_eq!(counts.take(), [2, 1, 1, 1]);
        assert_eq!(counts.take(), [0; 4]);
    }
}
