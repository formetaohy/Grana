use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Obj,
    Stl,
    Glb,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Obj => "obj",
            Self::Stl => "stl",
            Self::Glb => "glb",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Obj => "Wavefront OBJ",
            Self::Stl => "STL",
            Self::Glb => "glTF binary",
        }
    }
}
