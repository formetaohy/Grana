pub struct Model;

impl Model {
    pub const NAME: &'static str = "hvit_t_cells";
    pub const FILE: &'static str = "hvit_t_cells.pt";
    pub const URL: &'static str = "https://owncloud.gwdg.de/index.php/s/KEtW0dobX2az0GO/download";
    pub const LENGTH: u64 = 134_597_252;
    pub const CHECKSUM: &'static str = "a469c3d0a5b65804c7858fc46f3f83ec";
}

pub(crate) struct Asset {
    pub(crate) url: &'static str,
    pub(crate) length: u64,
    pub(crate) checksum: &'static str,
}

pub(crate) fn asset() -> Asset {
    Asset {
        url: Model::URL,
        length: Model::LENGTH,
        checksum: Model::CHECKSUM,
    }
}
