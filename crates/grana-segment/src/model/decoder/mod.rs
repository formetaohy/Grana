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
use crate::model::decoder::sums::{ColumnSums, DepthSums, accumulate};

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
const LEVEL_SHAPE: [(usize, usize); LEVELS] = [
    (FEATURES[0], 64),
    (FEATURES[0], 128),
    (FEATURES[1], 256),
    (FEATURES[1], 128),
    (FEATURES[2], 256),
    (FEATURES[2], 512),
    (FEATURES[3], 512),
];

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
    parts: Vec<Tensor<5>>,
    shape: [usize; 3],
}

impl Level {
    fn open(side: usize, splits: usize, channels: usize) -> Self {
        Self {
            parts: Vec::new(),
            spans: stripe(side, splits).spans(),
            shape: [channels, side, side],
        }
    }

    fn fill(&mut self, parts: Vec<Tensor<5>>) {
        assert_eq!(
            parts.len(),
            self.spans.len(),
            "a decoded level holds one tensor per span"
        );
        self.parts = parts;
    }

    fn release(&mut self) {
        self.parts.clear();
    }

    fn side(&self) -> usize {
        self.shape[2]
    }

    fn gather(&self, x: Range<usize>) -> Tensor<5> {
        let mut pieces = Vec::new();
        for (position, span) in self.spans.iter().enumerate() {
            let start = span.start().max(x.start);
            let end = (span.start() + span.extent()).min(x.end);
            if start >= end {
                continue;
            }
            let part = self.parts.get(position).unwrap_or_else(|| {
                panic!("a decoded level holds every span it covers, missing {position}")
            });
            pieces.push(part.clone().narrow(4, start - span.start(), end - start));
        }
        assert!(!pieces.is_empty(), "a gathered window overlaps a span");
        assert_eq!(
            pieces.iter().map(|piece| piece.dims()[4]).sum::<usize>(),
            x.end - x.start,
            "a gathered window covers its requested width"
        );
        if pieces.len() == 1 {
            return pieces.into_iter().next().expect("one window piece");
        }
        Tensor::cat(pieces, 4)
    }
}

struct Stream {
    levels: Vec<Level>,
}

impl Stream {
    fn open(splits: usize) -> Self {
        Self {
            levels: LEVEL_SHAPE
                .iter()
                .map(|(channels, side)| Level::open(*side, splits, *channels))
                .collect(),
        }
    }

    fn release(&mut self, level: usize) {
        self.levels[level].release();
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
            readout: readout.narrow_outputs(0, OUTPUT_CHANNELS),
        })
    }

    pub(crate) fn decode(
        &self,
        features: &dyn Fn(Range<usize>) -> Result<Tensor<5>>,
        depth: usize,
        splits: usize,
        emit: &mut dyn FnMut(usize, Tensor<5>) -> Result<()>,
    ) -> Result<()> {
        let device = self.readout.device();
        prime_pool(&device, workspace(depth, splits));
        let mut stream = Stream::open(splits);
        self.pass(features, depth, splits, &mut stream, emit)
    }

    fn pass(
        &self,
        features: &dyn Fn(Range<usize>) -> Result<Tensor<5>>,
        depth: usize,
        splits: usize,
        stream: &mut Stream,
        emit: &mut dyn FnMut(usize, Tensor<5>) -> Result<()>,
    ) -> Result<()> {
        let side = 64;
        let input = features(0..depth)?;
        assert_eq!(
            input.dims()[2],
            depth,
            "the encoder features cover the whole decoded region"
        );
        let source = narrow_source(&input);
        stream.levels[BASE].fill(
            self.base
                .split(&source, stripe(side, splits))
                .into_tensors(),
        );
        stream.levels[Z9].fill(
            self.deconv[0]
                .split(&source, stripe(side * 2, splits))
                .into_tensors(),
        );
        drop(source);
        drop(input);
        self.produce_deconv(stream, Z6, Z9, 1, side * 4, splits);
        self.produce_block(stream, X0, [BASE, Z9], 0, side * 2, splits);
        stream.release(BASE);
        stream.release(Z9);
        self.produce_block(stream, X1, [X0, Z6], 1, side * 4, splits);
        stream.release(X0);
        self.produce_deconv(stream, Z3, Z6, 2, side * 8, splits);
        stream.release(Z6);
        self.produce_block(stream, X2, [X1, Z3], 2, side * 8, splits);
        stream.release(X1);
        self.emit_pass(stream, splits, emit)?;
        stream.release(X2);
        stream.release(Z3);
        Ok(())
    }

    fn produce_deconv(
        &self,
        stream: &mut Stream,
        level: usize,
        upstream: usize,
        index: usize,
        side: usize,
        splits: usize,
    ) {
        let source = stream.levels[upstream].clone();
        let source = move |x: Range<usize>| source.gather(x);
        let parts = self.deconv[index]
            .split(&source, stripe(side, splits))
            .into_tensors();
        stream.levels[level].fill(parts);
    }

    fn produce_block(
        &self,
        stream: &mut Stream,
        level: usize,
        inputs: [usize; 2],
        index: usize,
        side: usize,
        splits: usize,
    ) {
        let sampled = stream.levels[inputs[0]].clone();
        let decoded = stream.levels[inputs[1]].clone();
        let sampler = &self.samplers[index];
        let channels = FEATURES[index + 1];
        let source = move |x: Range<usize>| {
            let raised_input = sample_gathered(sampler, &sampled, x.clone());
            let decoder_input = center_channels(decoded.gather(x.clone()), channels);
            Tensor::cat(vec![raised_input, decoder_input], 1)
        };
        let parts = self.blocks[index]
            .split(&source, stripe(side, splits))
            .into_tensors();
        stream.levels[level].fill(parts);
    }

    fn emit_pass(
        &self,
        stream: &Stream,
        splits: usize,
        emit: &mut dyn FnMut(usize, Tensor<5>) -> Result<()>,
    ) -> Result<()> {
        let side = 64 * 16;
        let stripe = stripe(side, splits);
        let x2 = stream.levels[X2].clone();
        let z3 = stream.levels[Z3].clone();
        let xout_of = move |x: Range<usize>| x2.gather(x);
        let z0_of = move |x: Range<usize>| z3.gather(x);

        let mut left_stats: Option<ColumnSums> = None;
        let mut right_stats: Option<ColumnSums> = None;
        for span in stripe.spans() {
            let xout = self.output.span(&xout_of, span, side);
            let z0 = self.deconv[3].span(&z0_of, span, side);
            accumulate(&mut left_stats, &xout, span.start());
            accumulate(&mut right_stats, &z0, span.start());
        }
        let left_stats = left_stats.expect("a stripe holds one span").statistics();
        let right_stats = right_stats.expect("a stripe holds one span").statistics();

        let mut sums: Option<ColumnSums> = None;
        for span in stripe.spans() {
            let wide = widened(span, side);
            let left = self.output.span(&xout_of, wide, side);
            let right = self.deconv[3].span(&z0_of, wide, side);
            let part = self.head.combine(left, right, &left_stats, &right_stats);
            accumulate(&mut sums, &span.crop(part, 1), span.start());
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
            let logits = self.readout.forward(gate.apply(refined), [0, 0, 0]);
            activated.push(activate(logits));
        }
        emit(0, joined(activated))
    }
}

fn widened(span: Span, side: usize) -> Span {
    let context = span.context(1);
    Span::new(context.start, context.end - context.start, side)
}

fn sample_gathered(sampler: &Sampler, level: &Level, x: Range<usize>) -> Tensor<5> {
    let parents = parent(&x, level.side());
    let gathered = level.gather(parents.clone());
    let sampled = sampler.forward(gathered);
    sampled.narrow(4, raised(&x, &parents), x.end - x.start)
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

fn plane(channels: usize, side: usize, depth: usize) -> u64 {
    channels as u64 * side as u64 * side as u64 * depth as u64 * size_of::<f32>() as u64
}

pub(crate) fn residency(depth: usize) -> u64 {
    LEVEL_SHAPE
        .iter()
        .map(|(channels, side)| plane(*channels, *side, depth))
        .sum()
}

fn span_width(side: usize, splits: usize) -> usize {
    let count = splits.min(side / SPAN_FLOOR).max(1);
    let extent = side / count;
    if count == 1 { extent } else { extent + 2 }
}

fn slab(channels: usize, side: usize, width: usize, depth: usize) -> u64 {
    channels as u64 * side as u64 * width as u64 * depth as u64 * size_of::<f32>() as u64
}

pub(crate) fn workspace(depth: usize, splits: usize) -> u64 {
    let widest = crate::plan::SIDE;
    let width = span_width(widest, splits);
    let upsampled = slab(FEATURES[3], widest * 2, width * 2, depth);
    let combined = slab(FEATURES[3] * 2, widest, width, depth);
    let deepest = slab(
        FEATURES[2],
        widest / 2,
        span_width(widest / 2, splits),
        depth,
    );
    upsampled.max(combined).max(deepest)
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
