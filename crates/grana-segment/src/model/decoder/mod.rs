mod conv_block;
mod deconv;
mod head;
mod parts;
mod span;
mod strip;
mod sums;

use std::collections::VecDeque;
use std::ops::Range;

use burn::prelude::*;
use burn::tensor::activation::{sigmoid, tanh};

use crate::Result;
use crate::model::checkpoint::{Checkpoint, Conv3d};
use crate::model::decoder::conv_block::ConvBlock;
use crate::model::decoder::deconv::{DeconvBlock, Sampler, joined};
use crate::model::decoder::head::Head;
use crate::model::decoder::span::{Span, Stripe, parent, raised};
use crate::model::decoder::strip::no_gate;
use crate::model::decoder::sums::{ColumnSums, DepthSums};

pub(crate) const SLAB: usize = 4;

const SPAN_FLOOR: usize = 32;

const EMBEDDING: usize = 256;
const FEATURES: [usize; 4] = [256, 128, 64, 32];
const OUTPUT_CHANNELS: usize = 4;
const READOUT_CHANNELS: usize = 5;

const BASE: usize = 0;
const Z9: usize = 1;
const Z6: usize = 2;
const X0: usize = 3;
const X1: usize = 4;
const Z3: usize = 5;
const X2: usize = 6;
const LEVELS: usize = 7;
const SHIFTS: [usize; LEVELS] = [8, 8, 6, 6, 4, 4, 2];

struct Pass {
    volume: usize,
    a: usize,
    b: usize,
    splits: usize,
}

pub(crate) struct Decoder {
    base: ConvBlock,
    deconv: [DeconvBlock; 4],
    blocks: [ConvBlock; 3],
    samplers: [Sampler; 3],
    output: DeconvBlock,
    head: Head,
    readout: Conv3d,
}

#[derive(Clone)]
struct Level {
    spans: Vec<Span>,
    chunks: VecDeque<Chunk>,
    shape: [usize; 3],
    device: Device,
    end: usize,
}

impl Level {
    fn open(side: usize, splits: usize, channels: usize, device: &Device) -> Self {
        let spans = stripe(side, splits).spans();
        Self {
            chunks: VecDeque::new(),
            spans,
            shape: [channels, side, side],
            device: device.clone(),
            end: 0,
        }
    }

    fn produce(&mut self, parts: Vec<Tensor<5>>, cover: Range<usize>) {
        assert_eq!(
            parts.len(),
            self.spans.len(),
            "a produced level holds one tensor per span"
        );
        if cover.is_empty() {
            return;
        }
        self.end = self.end.max(cover.end);
        self.chunks.push_back(Chunk::of(parts, cover));
    }

    fn end(&self) -> usize {
        self.end
    }

    fn side(&self) -> usize {
        self.spans
            .iter()
            .map(|span| span.start() + span.extent())
            .max()
            .unwrap_or(0)
    }

    fn gather(&self, z: Range<isize>, x: Range<usize>, volume: usize) -> Tensor<5> {
        let depth = (z.end - z.start).max(0) as usize;
        let mut pieces = Vec::new();
        for (position, span) in self.spans.iter().enumerate() {
            let start = span.start().max(x.start);
            let end = (span.start() + span.extent()).min(x.end);
            if start >= end {
                continue;
            }
            let mut chunk_parts = Vec::new();
            let mut covers = Vec::new();
            for chunk in &self.chunks {
                if let Some(part) = chunk.parts.get(position) {
                    chunk_parts.push(part.clone().narrow(4, start - span.start(), end - start));
                    covers.push(chunk.cover.clone());
                }
            }
            pieces.push(self.zwindow(&chunk_parts, &covers, end - start, z.clone(), depth, volume));
        }
        assert!(!pieces.is_empty(), "a gathered window overlaps a span");
        joined(pieces)
    }

    fn zwindow(
        &self,
        chunks: &[Tensor<5>],
        covers: &[Range<usize>],
        xwidth: usize,
        z: Range<isize>,
        depth: usize,
        volume: usize,
    ) -> Tensor<5> {
        let [channels, height, _] = self.shape;
        let zeros = |len: usize| -> Option<Tensor<5>> {
            if len == 0 {
                return None;
            }
            Some(
                Tensor::<5>::zeros([1, channels, 1, height, xwidth], &self.device)
                    .expand([1, channels, len, height, xwidth]),
            )
        };
        let mut pieces: Vec<Tensor<5>> = Vec::new();
        let mut zpos = z.start;
        for (chunk, cover) in chunks.iter().zip(covers) {
            if cover.is_empty() {
                continue;
            }
            let lo = (cover.start as isize).max(zpos).max(z.start);
            let hi = (cover.end as isize).min(z.end);
            if hi <= lo {
                continue;
            }
            if lo > zpos {
                assert!(
                    lo <= 0,
                    "a gathered window never reads missing interior slices: cover {cover:?} against {z:?} (zpos={zpos}, lo={lo})"
                );
                if let Some(padding) = zeros((lo - zpos) as usize) {
                    pieces.push(padding);
                }
            }
            let offset = (lo - cover.start as isize) as usize;
            pieces.push(chunk.clone().narrow(2, offset, (hi - lo) as usize));
            zpos = hi;
        }
        let used = pieces.iter().map(|piece| piece.dims()[2]).sum::<usize>();
        if used < depth {
            assert!(
                zpos >= volume as isize,
                "a gathered window never pads missing trailing interior slices: zpos={zpos} volume={volume} z={z:?}"
            );
            if let Some(padding) = zeros(depth - used) {
                pieces.push(padding);
            }
        }
        assert_eq!(
            pieces.iter().map(|piece| piece.dims()[2]).sum::<usize>(),
            depth,
            "a gathered window covers its requested depth"
        );
        if pieces.len() == 1 {
            return pieces.into_iter().next().expect("one window piece");
        }
        Tensor::cat(pieces, 2)
    }

    fn trim_before(&mut self, floor: usize) {
        while let Some(front) = self.chunks.front() {
            if front.cover.end <= floor {
                self.chunks.pop_front();
            } else {
                break;
            }
        }
        if let Some(front) = self.chunks.front_mut()
            && front.cover.start < floor
        {
            let len = front.cover.end - floor;
            let drop = front.cover.len() - len;
            *front = front.compact_tail(drop, len);
        }
    }
}

#[derive(Clone)]
struct Chunk {
    parts: Vec<Tensor<5>>,
    cover: Range<usize>,
}

impl Chunk {
    fn of(parts: Vec<Tensor<5>>, cover: Range<usize>) -> Self {
        Self { parts, cover }
    }

    fn compact_tail(&self, drop: usize, keep: usize) -> Self {
        Self {
            parts: self
                .parts
                .iter()
                .map(|part| compact(part.clone().narrow(2, drop, keep)))
                .collect(),
            cover: self.cover.start + drop..self.cover.end,
        }
    }
}

fn compact(tensor: Tensor<5>) -> Tensor<5> {
    tensor + 0.0
}

struct Stream {
    levels: Vec<Level>,
}

impl Stream {
    fn open(splits: usize, device: &Device) -> Self {
        let side = 64;
        let levels = vec![
            Level::open(side, splits, FEATURES[0], device),
            Level::open(side * 2, splits, FEATURES[0], device),
            Level::open(side * 4, splits, FEATURES[1], device),
            Level::open(side * 2, splits, FEATURES[1], device),
            Level::open(side * 4, splits, FEATURES[2], device),
            Level::open(side * 8, splits, FEATURES[2], device),
            Level::open(side * 8, splits, FEATURES[3], device),
        ];
        Self { levels }
    }

    fn trim_level(&mut self, level: usize, floor: usize) {
        self.levels[level].trim_before(floor);
    }
}

impl Decoder {
    pub(crate) fn load(checkpoint: &Checkpoint, device: &Device) -> Result<Self> {
        let mut deconv = Vec::with_capacity(4);
        for index in 0..4 {
            let input = if index == 0 {
                EMBEDDING
            } else {
                FEATURES[index - 1]
            };
            deconv.push(DeconvBlock::load(
                checkpoint,
                &format!("deconv{}", index + 1),
                input,
                FEATURES[index],
                device,
            )?);
        }
        let mut blocks = Vec::with_capacity(3);
        let mut samplers = Vec::with_capacity(3);
        for index in 0..3 {
            blocks.push(ConvBlock::load(
                checkpoint,
                &format!("decoder.blocks.{index}"),
                FEATURES[index + 1] * 2,
                FEATURES[index + 1],
                device,
            )?);
            samplers.push(Sampler::load(
                checkpoint,
                &format!("decoder.samplers.{index}"),
                FEATURES[index],
                FEATURES[index + 1],
                device,
            )?);
        }
        let deconv: [DeconvBlock; 4] = deconv
            .try_into()
            .map_err(|_| crate::error::Error::model("the decoder needs four deconvolutions"))?;
        let blocks: [ConvBlock; 3] = blocks
            .try_into()
            .map_err(|_| crate::error::Error::model("the decoder needs three blocks"))?;
        let samplers: [Sampler; 3] = samplers
            .try_into()
            .map_err(|_| crate::error::Error::model("the decoder needs three samplers"))?;
        let readout = Conv3d::load(
            checkpoint,
            "out_conv.weight",
            "out_conv.bias",
            [READOUT_CHANNELS, FEATURES[3], 1, 1, 1],
            device,
        )?;
        Ok(Self {
            base: ConvBlock::load(checkpoint, "base", EMBEDDING, FEATURES[0], device)?,
            deconv,
            blocks,
            samplers,
            output: DeconvBlock::load(checkpoint, "deconv_out", FEATURES[3], FEATURES[3], device)?,
            head: Head::load(checkpoint, FEATURES[3], device)?,
            readout: Conv3d {
                weight: readout.weight.narrow(0, 0, OUTPUT_CHANNELS),
                bias: readout.bias.narrow(0, 0, OUTPUT_CHANNELS),
            },
        })
    }

    pub(crate) fn decode(
        &self,
        features: &dyn Fn(Range<usize>) -> Result<Tensor<5>>,
        depth: usize,
        splits: usize,
        emit: &mut dyn FnMut(usize, Tensor<5>) -> Result<()>,
    ) -> Result<()> {
        let device = self.readout.weight.device();
        prime_pool(&device, max_tensor_bytes(depth, splits));
        let mut stream = Stream::open(splits, &device);
        let mut start = 0;
        while start < depth {
            let end = depth.min(start + SLAB);
            let pass = Pass {
                volume: depth,
                a: start,
                b: end,
                splits,
            };
            self.pass(features, &pass, &mut stream, emit)?;
            start = end;
        }
        Ok(())
    }

    fn target(&self, stream: &Stream, level: usize, pass: &Pass) -> Range<usize> {
        stream.levels[level].end()..(pass.b + SHIFTS[level]).min(pass.volume)
    }

    fn pass(
        &self,
        features: &dyn Fn(Range<usize>) -> Result<Tensor<5>>,
        pass: &Pass,
        stream: &mut Stream,
        emit: &mut dyn FnMut(usize, Tensor<5>) -> Result<()>,
    ) -> Result<()> {
        let side = 64;
        let splits = pass.splits;

        let target = self.target(stream, BASE, pass);
        if !target.is_empty() {
            let z = target.start as isize - 2..target.end as isize + 2;
            let input = feature_window(features, z, pass.volume)?;
            let source = narrow_source(&input);
            let parts = self
                .base
                .split(&source, stripe(side, splits))
                .into_tensors();
            stream.levels[BASE].produce(parts, target);
        }

        let target = self.target(stream, Z9, pass);
        if !target.is_empty() {
            let z = target.start as isize..target.end as isize;
            let input = feature_window(features, z, pass.volume)?;
            let source = narrow_source(&input);
            let parts = self.deconv[0]
                .split(&source, stripe(side * 2, splits))
                .into_tensors();
            stream.levels[Z9].produce(parts, target);
        }

        self.produce_deconv(stream, pass, Z6, Z9, 1, side * 4);
        self.produce_block(stream, pass, X0, [BASE, Z9], 0, side * 2);
        let floor_base = if stream.levels[X0].end() < pass.volume {
            stream.levels[X0].end().saturating_sub(2)
        } else {
            pass.volume
        };
        let floor_z9 = floor_base.min(if stream.levels[Z6].end() < pass.volume {
            stream.levels[Z6].end()
        } else {
            pass.volume
        });
        stream.trim_level(BASE, floor_base);
        stream.trim_level(Z9, floor_z9);

        self.produce_block(stream, pass, X1, [X0, Z6], 1, side * 4);
        let floor_x0 = if stream.levels[X1].end() < pass.volume {
            stream.levels[X1].end().saturating_sub(2)
        } else {
            pass.volume
        };
        stream.trim_level(X0, floor_x0);

        self.produce_deconv(stream, pass, Z3, Z6, 2, side * 8);
        let floor_z6 = floor_x0.min(if stream.levels[Z3].end() < pass.volume {
            stream.levels[Z3].end()
        } else {
            pass.volume
        });
        stream.trim_level(Z6, floor_z6);

        self.produce_block(stream, pass, X2, [X1, Z3], 2, side * 8);
        let floor_x1 = if stream.levels[X2].end() < pass.volume {
            stream.levels[X2].end().saturating_sub(2)
        } else {
            pass.volume
        };
        stream.trim_level(X1, floor_x1);

        self.emit_pass(stream, pass, emit)?;
        let floor_emit = if pass.b < pass.volume {
            pass.b.saturating_sub(2)
        } else {
            pass.volume
        };
        let floor_z3 = floor_emit.min(floor_x1);
        stream.trim_level(Z3, floor_z3);
        stream.trim_level(X2, floor_emit);
        Ok(())
    }

    fn produce_deconv(
        &self,
        stream: &mut Stream,
        pass: &Pass,
        level: usize,
        upstream: usize,
        index: usize,
        side: usize,
    ) {
        let target = self.target(stream, level, pass);
        if target.is_empty() {
            return;
        }
        let z = target.start as isize..target.end as isize;
        let volume = pass.volume;
        let upstream = stream.levels[upstream].clone();
        let source = move |x: Range<usize>| upstream.gather(z.clone(), x, volume);
        let parts = self.deconv[index]
            .split(&source, stripe(side, pass.splits))
            .into_tensors();
        stream.levels[level].produce(parts, target);
    }

    fn produce_block(
        &self,
        stream: &mut Stream,
        pass: &Pass,
        level: usize,
        inputs: [usize; 2],
        index: usize,
        side: usize,
    ) {
        let target = self.target(stream, level, pass);
        if target.is_empty() {
            return;
        }
        let z = target.start as isize - 2..target.end as isize + 2;
        let volume = pass.volume;
        let sampled = stream.levels[inputs[0]].clone();
        let decoded = stream.levels[inputs[1]].clone();
        let sampler = &self.samplers[index];
        let channels = FEATURES[index + 1];
        let source = move |x: Range<usize>| {
            let raised_input = sample_gathered(sampler, &sampled, z.clone(), x.clone(), volume);
            let decoder_input =
                center_channels(decoded.gather(z.clone(), x.clone(), volume), channels);
            Tensor::cat(vec![raised_input, decoder_input], 1)
        };
        let parts = self.blocks[index]
            .split(&source, stripe(side, pass.splits))
            .into_tensors();
        stream.levels[level].produce(parts, target);
    }

    fn emit_pass(
        &self,
        stream: &Stream,
        pass: &Pass,
        emit: &mut dyn FnMut(usize, Tensor<5>) -> Result<()>,
    ) -> Result<()> {
        let a = pass.a;
        let b = pass.b;
        let side = 64 * 16;
        let stripe = stripe(side, pass.splits);
        let z = a as isize - 2..b as isize + 2;

        let x2 = stream.levels[X2].clone();
        let z3 = stream.levels[Z3].clone();
        let volume = pass.volume;
        let zw = z.clone();
        let xout_of = move |x: Range<usize>| x2.gather(zw.clone(), x, volume);
        let z0_of = move |x: Range<usize>| z3.gather(z.clone(), x, volume);

        let mut left_stats: Option<ColumnSums> = None;
        let mut right_stats: Option<ColumnSums> = None;
        for span in stripe.spans() {
            let xout = self.output.span(&xout_of, span, side);
            let z0 = self.deconv[3].span(&z0_of, span, side);
            accumulate(&mut left_stats, &xout);
            accumulate(&mut right_stats, &z0);
        }
        let left_stats = left_stats.expect("a stripe holds one span").statistics();
        let right_stats = right_stats.expect("a stripe holds one span").statistics();

        let mut sums: Option<ColumnSums> = None;
        for span in stripe.spans() {
            let wide = widened(span, side);
            let left = self.output.span(&xout_of, wide, side);
            let right = self.deconv[3].span(&z0_of, wide, side);
            let part = self.head.combine(left, right, &left_stats, &right_stats);
            accumulate(&mut sums, &span.crop(part, 1));
        }
        let combined_stats = sums.expect("a stripe holds one span").statistics();

        let mut activated: Vec<Tensor<5>> = Vec::new();
        for span in stripe.spans() {
            let wide = widened(span, side);
            let left = self.output.span(&xout_of, wide, side);
            let right = self.deconv[3].span(&z0_of, wide, side);
            let part = self.head.combine(left, right, &left_stats, &right_stats);
            let refined = span.crop(self.head.refine(part, &combined_stats), 1);
            let mut depth_sums = DepthSums::of(&refined);
            depth_sums.add(&refined);
            let gate = if depth_sums.depth() <= 1 {
                no_gate()
            } else {
                self.head.gate(depth_sums.mean())
            };
            let logits = self
                .readout
                .forward(gate.apply(refined), [0, 0, 0], [1, 1, 1]);
            activated.push(activate(logits));
        }
        let activated = joined(activated);
        emit(a, activated)?;
        Ok(())
    }
}

fn widened(span: Span, side: usize) -> Span {
    let context = span.context(1);
    Span::new(context.start, context.end - context.start, side)
}

fn sample_gathered(
    sampler: &Sampler,
    level: &Level,
    z: Range<isize>,
    x: Range<usize>,
    volume: usize,
) -> Tensor<5> {
    let parents = parent(&x, level.side());
    let gathered = level.gather(z, parents.clone(), volume);
    let sampled = sampler.forward(gathered);
    sampled.narrow(4, raised(&x, &parents), x.end - x.start)
}

fn feature_window(
    features: &dyn Fn(Range<usize>) -> Result<Tensor<5>>,
    z: Range<isize>,
    volume: usize,
) -> Result<Tensor<5>> {
    let depth = (z.end - z.start).max(0) as usize;
    let lo = z.start.max(0).min(volume as isize);
    let hi = z.end.min(volume as isize).max(0);
    let real = features(lo as usize..hi as usize)?;
    let [batch, channels, real_depth, height, width] = real.dims();
    let device = real.device();
    let zeros = |len: usize| -> Option<Tensor<5>> {
        if len == 0 {
            return None;
        }
        Some(
            Tensor::<5>::zeros([batch, channels, 1, height, width], &device)
                .expand([batch, channels, len, height, width]),
        )
    };
    let mut pieces: Vec<Tensor<5>> = Vec::new();
    let lead = (lo - z.start) as usize;
    if let Some(padding) = zeros(lead) {
        pieces.push(padding);
    }
    pieces.push(real);
    let used = lead + real_depth;
    if used < depth
        && let Some(padding) = zeros(depth - used)
    {
        pieces.push(padding);
    }
    if pieces.len() == 1 {
        return Ok(pieces.into_iter().next().expect("one window piece"));
    }
    Ok(Tensor::cat(pieces, 2))
}

fn narrow_source(input: &Tensor<5>) -> impl Fn(Range<usize>) -> Tensor<5> + '_ {
    move |range: Range<usize>| {
        input
            .clone()
            .narrow(4, range.start, range.end - range.start)
    }
}

fn prime_pool(device: &Device, bytes: u64) {
    let elements = bytes.div_ceil(size_of::<f32>() as u64) as usize;
    if elements == 0 {
        return;
    }
    let _ = Tensor::<1>::zeros([elements], device)
        .narrow(0, 0, 1)
        .into_data();
}

pub(crate) fn max_tensor_bytes(depth: usize, splits: usize) -> u64 {
    let bytes = |channels: usize, slices: usize, height: usize, width: usize| -> u64 {
        channels as u64 * slices as u64 * height as u64 * width as u64 * size_of::<f32>() as u64
    };
    let span_width = |side: usize, halo: usize| -> usize {
        let count = splits.min(side / SPAN_FLOOR).max(1);
        let extent = side / count;
        if count == 1 {
            extent
        } else if count == 2 {
            extent + halo
        } else {
            extent + 2 * halo
        }
    };
    let deconv_width = |side: usize, widened: bool| -> usize {
        let count = splits.min(side / SPAN_FLOOR).max(1);
        let extent = side / count;
        let target_start = if count > 1 { extent } else { 0 };
        let target_end = (target_start + extent).min(side);
        let target = if widened {
            target_start.saturating_sub(1)..(target_end + 1).min(side)
        } else {
            target_start..target_end
        };
        let parents = parent(&target, side / 2);
        (parents.end - parents.start) * 2
    };
    let produced = |shift: usize| -> usize { (SLAB + shift).min(depth) };
    let encoder = 36u64 << 20;
    let base = bytes(256, produced(SHIFTS[BASE]) + 4, 64, span_width(64, 1));
    let z9 = bytes(256, produced(SHIFTS[Z9]), 128, deconv_width(128, false));
    let z6 = bytes(128, produced(SHIFTS[Z6]), 256, deconv_width(256, false));
    let x0 = bytes(256, produced(SHIFTS[X0]) + 4, 128, span_width(128, 1));
    let x1 = bytes(128, produced(SHIFTS[X1]) + 4, 256, span_width(256, 1));
    let z3 = bytes(64, produced(SHIFTS[Z3]), 512, deconv_width(512, false));
    let x2 = bytes(64, produced(SHIFTS[X2]) + 4, 512, span_width(512, 1));
    let emit = bytes(32, depth.min(SLAB) + 4, 1024, deconv_width(1024, true));
    [encoder, base, z9, z6, x0, x1, z3, x2, emit]
        .into_iter()
        .max()
        .unwrap_or(encoder)
}

fn stripe(side: usize, splits: usize) -> Stripe {
    Stripe::new(side, splits.min(side / SPAN_FLOOR).max(1))
}

fn center_channels(input: Tensor<5>, channels: usize) -> Tensor<5> {
    let current = input.dims()[1];
    if current == channels {
        return input;
    }
    input.narrow(1, (current - channels) / 2, channels)
}

fn activate(logits: Tensor<5>) -> Tensor<5> {
    let foreground = sigmoid(logits.clone().narrow(1, 0, 1));
    let distances = tanh(logits.clone().narrow(1, 1, 3));
    Tensor::cat(vec![foreground, distances], 1)
}

fn accumulate(holder: &mut Option<ColumnSums>, tensor: &Tensor<5>) {
    let mut sums = ColumnSums::of(tensor);
    sums.add(tensor);
    *holder = Some(match holder.take() {
        None => sums,
        Some(held) => held.join(sums),
    });
}
