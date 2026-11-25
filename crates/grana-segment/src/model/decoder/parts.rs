use std::ops::Range;

use burn::prelude::*;

use crate::model::decoder::span::{Span, Stripe};

pub(crate) struct Parts {
    spans: Vec<Span>,
    tensors: Vec<Tensor<5>>,
}

impl Parts {
    pub(crate) fn new(spans: Vec<Span>, tensors: Vec<Tensor<5>>) -> Self {
        assert!(
            spans.len() == tensors.len() && !spans.is_empty(),
            "a decoded level holds one tensor per span"
        );
        Self { spans, tensors }
    }

    pub(crate) fn side(&self) -> usize {
        self.spans
            .iter()
            .map(|span| span.start() + span.extent())
            .max()
            .unwrap_or(0)
    }

    pub(crate) fn source(&self, range: &Range<usize>) -> Tensor<5> {
        let mut parts = Vec::new();
        for (span, tensor) in self.spans.iter().zip(&self.tensors) {
            let start = span.start().max(range.start);
            let end = (span.start() + span.extent()).min(range.end);
            if start >= end {
                continue;
            }
            parts.push(tensor.clone().narrow(4, start - span.start(), end - start));
        }
        joined(parts)
    }

    pub(crate) fn into_tensors(self) -> Vec<Tensor<5>> {
        self.tensors
    }
}

pub(crate) fn split(stripe: Stripe, tensors: impl IntoIterator<Item = Tensor<5>>) -> Parts {
    Parts::new(stripe.spans(), tensors.into_iter().collect())
}

fn joined(parts: Vec<Tensor<5>>) -> Tensor<5> {
    if parts.len() == 1 {
        return parts.into_iter().next().expect("one decoded part");
    }
    Tensor::cat(parts, 4)
}
