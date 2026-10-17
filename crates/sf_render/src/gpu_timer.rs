//! GPU frame time from timestamp queries (WP1.14, `sf_demo --bench`).
//!
//! Two timestamps bracket all the commands of one frame (scene, UI and
//! overlay passes). They are resolved into a buffer, copied into one of a
//! few read-back buffers and mapped without waiting: the result of a frame
//! shows up two or three frames later, when [`GpuTimer::poll`] finds the
//! mapping done. Nothing here ever blocks on the GPU, so measuring does not
//! change the frame it measures (apart from the few commands it records).
//!
//! Needs the device features `TIMESTAMP_QUERY` and
//! `TIMESTAMP_QUERY_INSIDE_ENCODERS` ([`crate::gpu::Gpu::timestamps`]); when
//! the adapter lacks them there is no timer and the benchmark says "n/a".

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::gpu::Gpu;

const MAPPING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

/// Frames that may be in flight at once; further frames are not measured
/// until a slot frees up (rare: the CPU would have to run far ahead).
const SLOTS: usize = 4;

struct Slot {
    buffer: wgpu::Buffer,
    /// Set by the map callback (`MAPPING` until then); `None` = free.
    pending: Option<Arc<AtomicU8>>,
    /// The caller's tag of the frame in this slot (see [`GpuTimer::submitted`]).
    tag: u64,
}

/// Measures the GPU time of whole frames.
pub struct GpuTimer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    slots: Vec<Slot>,
    /// Nanoseconds per timestamp tick.
    period_ns: f32,
    /// Finished measurements: the tag of the frame and its GPU time in
    /// milliseconds, oldest first.
    done: Vec<(u64, f64)>,
}

impl GpuTimer {
    /// A timer, or `None` when the device cannot do timestamp queries.
    pub fn new(gpu: &Gpu) -> Option<Self> {
        if !gpu.timestamps {
            return None;
        }
        let device = &gpu.device;
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("frame timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamp resolve"),
            size: 16,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = (0..SLOTS)
            .map(|_| Slot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("timestamp readback"),
                    size: 16,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                pending: None,
                tag: 0,
            })
            .collect();
        Some(Self {
            queries,
            resolve,
            slots,
            period_ns: gpu.queue.get_timestamp_period(),
            done: Vec::new(),
        })
    }

    /// Write the start timestamp. Returns the slot to hand to [`Self::end`],
    /// or `None` when all slots are busy (this frame is not measured).
    pub fn begin(&mut self, encoder: &mut wgpu::CommandEncoder) -> Option<usize> {
        let slot = self.slots.iter().position(|s| s.pending.is_none())?;
        encoder.write_timestamp(&self.queries, 0);
        Some(slot)
    }

    /// Write the end timestamp and queue the copy into the slot's buffer.
    pub fn end(&mut self, encoder: &mut wgpu::CommandEncoder, slot: usize) {
        encoder.write_timestamp(&self.queries, 1);
        encoder.resolve_query_set(&self.queries, 0..2, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.slots[slot].buffer, 0, 16);
    }

    /// Start mapping the slot's buffer; call after the frame was submitted.
    /// `tag` is the caller's number for this frame; it comes back with the
    /// result, which is several frames later (see [`Self::take`]).
    pub fn submitted(&mut self, slot: usize, tag: u64) {
        let ready = Arc::new(AtomicU8::new(MAPPING));
        let flag = ready.clone();
        self.slots[slot]
            .buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                flag.store(
                    if result.is_ok() { MAPPED } else { FAILED },
                    Ordering::Release,
                );
            });
        self.slots[slot].pending = Some(ready);
        self.slots[slot].tag = tag;
    }

    /// Collect the frames whose mapping has finished (the caller polls the
    /// device without waiting first).
    pub fn collect(&mut self) {
        for slot in &mut self.slots {
            let Some(ready) = &slot.pending else { continue };
            let state = ready.load(Ordering::Acquire);
            if state == MAPPING {
                continue;
            }
            if state == MAPPED
                && let Ok(range) = slot.buffer.slice(..).get_mapped_range()
            {
                let word = |i: usize| {
                    let mut b = [0_u8; 8];
                    b.copy_from_slice(&range[i * 8..i * 8 + 8]);
                    u64::from_le_bytes(b)
                };
                let (start, end) = (word(0), word(1));
                if end > start {
                    self.done.push((
                        slot.tag,
                        (end - start) as f64 * f64::from(self.period_ns) / 1e6,
                    ));
                }
                drop(range);
                slot.buffer.unmap();
            }
            slot.pending = None;
        }
    }

    /// The measurements finished since the last call: the tag given to
    /// [`Self::submitted`] for the frame, and its GPU time in milliseconds.
    pub fn take(&mut self) -> Vec<(u64, f64)> {
        std::mem::take(&mut self.done)
    }
}
