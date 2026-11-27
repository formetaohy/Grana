use burn::prelude::*;

use crate::model::decoder::span::Stripe;

pub(crate) struct Parts {
    tensors: Vec<Tensor<5>>,
}

impl Parts {
    pub(crate) fn into_tensors(self) -> Vec<Tensor<5>> {
        self.tensors
    }
}

pub(crate) fn split(stripe: Stripe, tensors: impl IntoIterator<Item = Tensor<5>>) -> Parts {
    let tensors: Vec<Tensor<5>> = tensors.into_iter().collect();
    assert_eq!(
        tensors.len(),
        stripe.spans().len(),
        "a decoded level holds one tensor per span"
    );
    Parts { tensors }
}
