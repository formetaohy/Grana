mod conv_block;
mod deconv;
mod head;
mod parts;
mod span;
mod strip;
mod sums;

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

const EMBEDDING: usize = 256;
const FEATURES: [usize; 4] = [256, 128, 64, 32];
const OUTPUT_CHANNELS: usize = 4;
const READOUT_CHANNELS: usize = 5;
const SPAN_FLOOR: usize = 64;

const BASE: usize = 0;
const Z9: usize = 1;
const Z6: usize = 2;
const X0: usize = 3;
const X1: usize = 4;
const Z3: usize = 5;
const X2: usize = 6;
const LEVELS: usize = 7;
pub(crate) const SHIFTS: [usize; LEVELS] = [8, 8, 6, 6, 4, 4, 2];
pub(crate) const RETENTION: [usize; LEVELS] = [4, 4, 4, 4, 4, 6, 4];

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
    chunks: [Chunk; 2],
    shape: [usize; 3],
    device: Device,
}

impl Level {
    fn open(side: usize, splits: usize, channels: usize, device: &Device) -> Self {
        let spans = stripe(side, splits).spans();
        Self {
            chunks: [Chunk::empty(), Chunk::empty()],
            spans,
            shape: [channels, side, side],
            device: device.clone(),
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
        self.chunks[0] = std::mem::replace(&mut self.chunks[1], Chunk::of(parts, cover));
    }

    fn end(&self) -> usize {
        self.chunks[1].cover.end.max(self.chunks[0].cover.end)
    }

    fn side(&self) -> usize {
        self.spans
            .iter()
            .map(|span| span.start() + span.extent())
            .max()
            .unwrap_or(0)
    }

    fn gather(&self, z: Range<isize>, x: Range<usize>) -> Tensor<5> {
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
            pieces.push(zwindow(
                &chunk_parts,
                &covers,
                self.shape,
                end - start,
                z.clone(),
                depth,
                &self.device,
            ));
        }
        assert!(!pieces.is_empty(), "a gathered window overlaps a span");
        joined(pieces)
    }

    fn trim(&mut self, keep: usize) {
        let latest = self.chunks[1].cover.clone();
        if latest.is_empty() {
            return;
        }
        let keep = keep.min(latest.len());
        let floor = latest.end - keep;
        let front = self.chunks[0].cover.clone();
        if front.end <= floor {
            self.chunks[0] = Chunk::empty();
        } else if front.start < floor {
            let len = front.end - floor;
            let drop = front.len() - len;
            self.chunks[0] = self.chunks[0].compact_tail(drop, len);
        }
        if latest.len() > keep {
            let drop = latest.len() - keep;
            self.chunks[1] = self.chunks[1].compact_tail(drop, keep);
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

    fn empty() -> Self {
        Self {
            parts: Vec::new(),
            cover: 0..0,
        }
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

fn zwindow(
    chunks: &[Tensor<5>],
    covers: &[Range<usize>],
    shape: [usize; 3],
    xwidth: usize,
    z: Range<isize>,
    depth: usize,
    device: &Device,
) -> Tensor<5> {
    let [channels, height, _] = shape;
    let zeros = |len: usize| -> Option<Tensor<5>> {
        if len == 0 {
            return None;
        }
        Some(
            Tensor::<5>::zeros([1, channels, 1, height, xwidth], device)
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
                zpos <= z.start,
                "a gathered window never reads interior holes: cover {cover:?} against {z:?}"
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
    if used < depth
        && let Some(padding) = zeros(depth - used)
    {
        pieces.push(padding);
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

    fn advance(&mut self) {
        for (index, level) in self.levels.iter_mut().enumerate() {
            level.trim(RETENTION[index]);
        }
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
        let mut stream = Stream::open(splits, &self.readout.weight.device());
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
            stream.advance();
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
        self.produce_block(stream, pass, X1, [X0, Z6], 1, side * 4);
        self.produce_deconv(stream, pass, Z3, Z6, 2, side * 8);
        self.produce_block(stream, pass, X2, [X1, Z3], 2, side * 8);

        self.emit_pass(stream, pass, emit)
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
        let upstream = stream.levels[upstream].clone();
        let source = move |x: Range<usize>| upstream.gather(z.clone(), x);
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
        let sampled = stream.levels[inputs[0]].clone();
        let decoded = stream.levels[inputs[1]].clone();
        let sampler = &self.samplers[index];
        let channels = FEATURES[index + 1];
        let source = move |x: Range<usize>| {
            let raised_input = sample_gathered(sampler, &sampled, z.clone(), x.clone());
            let decoder_input = center_channels(decoded.gather(z.clone(), x.clone()), channels);
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
        let zw = z.clone();
        let xout_of = move |x: Range<usize>| x2.gather(zw.clone(), x);
        let z0_of = move |x: Range<usize>| z3.gather(z.clone(), x);

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
) -> Tensor<5> {
    let parents = parent(&x, level.side());
    let gathered = level.gather(z, parents.clone());
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
