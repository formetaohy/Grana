use std::path::Path;

use burn::store::pytorch::PytorchReader;

use crate::Result;
use crate::error::Error;

pub(crate) const PATCH_WEIGHT: &str = "encoder.patch_embed.proj.weight";
pub(crate) const PATCH_BIAS: &str = "encoder.patch_embed.proj.bias";
pub(crate) const POSITION: &str = "encoder.pos_embed";
pub(crate) const OUT_WEIGHT: &str = "out.weight";
pub(crate) const OUT_BIAS: &str = "out.bias";
pub(crate) const UPSAMPLE: &str = "W2";
pub(crate) const BLOCKS: &str = "encoder.blocks.";

pub(crate) struct Checkpoint {
    reader: PytorchReader,
}

impl Checkpoint {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let reader = PytorchReader::new(path).map_err(|error| {
            Error::model(format!(
                "the weights `{}` cannot be read: {error}",
                path.display()
            ))
        })?;
        Ok(Self { reader })
    }

    pub(crate) fn contains(&self, name: &str) -> bool {
        self.reader.get(name).is_some()
    }

    pub(crate) fn shape(&self, name: &str) -> Result<Vec<usize>> {
        let tensor = self.reader.get(name).ok_or_else(|| missing(name))?;
        Ok(tensor.shape().to_vec())
    }

    pub(crate) fn values(&self, name: &str) -> Result<Vec<f32>> {
        let tensor = self.reader.get(name).ok_or_else(|| missing(name))?;
        let bytes = tensor.read().map_err(|error| {
            Error::model(format!("the weights `{name}` cannot be read: {error}"))
        })?;
        let count = tensor.num_elements();
        match bytes.len().checked_div(count) {
            Some(4) => Ok(bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|word| f32::from_le_bytes(*word))
                .collect()),
            Some(2) => Ok(bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|word| half::f16::from_bits(u16::from_le_bytes(*word)).to_f32())
                .collect()),
            _ => Err(Error::model(format!(
                "the weights `{name}` hold {} bytes for {count} values",
                bytes.len()
            ))),
        }
    }

    pub(crate) fn block_names(&self, index: usize) -> BlockNames {
        let prefix = format!("{BLOCKS}{index}.");
        BlockNames {
            norm_one_weight: format!("{prefix}norm1.weight"),
            norm_one_bias: format!("{prefix}norm1.bias"),
            qkv_weight: format!("{prefix}attn.qkv.weight"),
            qkv_bias: format!("{prefix}attn.qkv.bias"),
            proj_weight: format!("{prefix}attn.proj.weight"),
            proj_bias: format!("{prefix}attn.proj.bias"),
            norm_two_weight: format!("{prefix}norm2.weight"),
            norm_two_bias: format!("{prefix}norm2.bias"),
            mlp_one_weight: format!("{prefix}mlp.lin1.weight"),
            mlp_one_bias: format!("{prefix}mlp.lin1.bias"),
            mlp_two_weight: format!("{prefix}mlp.lin2.weight"),
            mlp_two_bias: format!("{prefix}mlp.lin2.bias"),
            row: format!("{prefix}attn.rel_pos_h"),
            column: format!("{prefix}attn.rel_pos_w"),
        }
    }

    pub(crate) fn block_count(&self) -> usize {
        let mut count = 0;
        while self.contains(&format!("{BLOCKS}{count}.norm1.weight")) {
            count += 1;
        }
        count
    }

    pub(crate) fn neck_names(&self) -> NeckNames {
        NeckNames {
            first_weight: "encoder.neck.0.weight".to_owned(),
            first_norm_weight: "encoder.neck.1.weight".to_owned(),
            first_norm_bias: "encoder.neck.1.bias".to_owned(),
            second_weight: "encoder.neck.2.weight".to_owned(),
            second_norm_weight: "encoder.neck.3.weight".to_owned(),
            second_norm_bias: "encoder.neck.3.bias".to_owned(),
        }
    }
}

pub(crate) struct BlockNames {
    pub(crate) norm_one_weight: String,
    pub(crate) norm_one_bias: String,
    pub(crate) qkv_weight: String,
    pub(crate) qkv_bias: String,
    pub(crate) proj_weight: String,
    pub(crate) proj_bias: String,
    pub(crate) norm_two_weight: String,
    pub(crate) norm_two_bias: String,
    pub(crate) mlp_one_weight: String,
    pub(crate) mlp_one_bias: String,
    pub(crate) mlp_two_weight: String,
    pub(crate) mlp_two_bias: String,
    pub(crate) row: String,
    pub(crate) column: String,
}

pub(crate) struct NeckNames {
    pub(crate) first_weight: String,
    pub(crate) first_norm_weight: String,
    pub(crate) first_norm_bias: String,
    pub(crate) second_weight: String,
    pub(crate) second_norm_weight: String,
    pub(crate) second_norm_bias: String,
}

fn missing(name: &str) -> Error {
    Error::model(format!("the weights hold no tensor named `{name}`"))
}
