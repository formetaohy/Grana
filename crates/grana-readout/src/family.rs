#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Nikon,
    Leica,
    Olympus,
    Zeiss,
    Imaris,
    Electron,
    Slide,
    Plate,
}

impl Family {
    pub(crate) fn of(id: &str) -> Option<Self> {
        match id {
            openreadout_nd2::FORMAT_ID => Some(Self::Nikon),
            openreadout_lif::FORMAT_ID => Some(Self::Leica),
            openreadout_hdf5::IMS_FORMAT_ID => Some(Self::Imaris),
            openreadout_oir::FORMAT_ID
            | openreadout_oif::OIF_FORMAT_ID
            | openreadout_oif::OIB_FORMAT_ID
            | openreadout_vsi::FORMAT_ID => Some(Self::Olympus),
            openreadout_zvi::FORMAT_ID => Some(Self::Zeiss),
            openreadout_em::dm::FORMAT_ID
            | openreadout_em::emd::FORMAT_ID
            | openreadout_em::mrc::FORMAT_ID
            | openreadout_em::ser::FORMAT_ID => Some(Self::Electron),
            openreadout_wsi::mirax::FORMAT_ID
            | openreadout_wsi::isyntax::FORMAT_ID
            | openreadout_wsi::vms::FORMAT_ID => Some(Self::Slide),
            openreadout_hcs::HARMONY_FORMAT_ID
            | openreadout_hcs::IMAGEXPRESS_FORMAT_ID
            | openreadout_hcs::CELLVOYAGER_FORMAT_ID => Some(Self::Plate),
            _ => None,
        }
    }
}
