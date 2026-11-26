use crate::{
    BrickCoord, BrickData, BrickGrid, Calibration, ChannelInfo, ColorTable, Extent, Intensity,
    Level, NoData, Pyramid, Spacing, Statistics, VoxelData, VoxelFormat, VoxelSemantics,
    brick::pad_brick,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    name: String,
    tint: [f32; 3],
    statistics: Statistics,
    data: VoxelData,
    no_data: Option<NoData>,
    color_table: Option<ColorTable>,
}

impl Channel {
    pub fn new(name: impl Into<String>, tint: [f32; 3], data: VoxelData) -> Self {
        assert!(
            tint.iter().all(|component| (0.0..=1.0).contains(component)),
            "a channel tint is an srgb triple"
        );
        let statistics = Statistics::census(&data, None);
        Self {
            name: name.into(),
            tint,
            statistics,
            data,
            no_data: None,
            color_table: None,
        }
    }

    pub fn with_color_table(mut self, color_table: ColorTable) -> Self {
        assert_eq!(
            self.data.format(),
            VoxelFormat::U8,
            "an indexed channel stores byte indices"
        );
        self.statistics = Statistics::declared(color_table.indices(), VoxelSemantics::Label)
            .with_background(self.statistics.background());
        self.color_table = Some(color_table);
        self
    }

    pub fn color_table(&self) -> Option<&ColorTable> {
        self.color_table.as_ref()
    }

    pub fn with_no_data(mut self, no_data: NoData) -> Self {
        assert_eq!(
            no_data.format(),
            self.data.format(),
            "a channel no-data value holds the format of its volume"
        );
        let declared = no_data.declared();
        if declared.is_some() {
            self.statistics = Statistics::census(&self.data, declared);
        }
        self.no_data = declared;
        self
    }

    pub fn no_data(&self) -> Option<NoData> {
        self.no_data
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn tint(&self) -> [f32; 3] {
        self.tint
    }

    pub fn intensity(&self) -> Intensity {
        self.statistics.intensity()
    }

    pub fn statistics(&self) -> Statistics {
        self.statistics
    }

    pub fn background(&self) -> f32 {
        self.statistics.background()
    }

    pub fn data(&self) -> &VoxelData {
        &self.data
    }

    pub fn format(&self) -> VoxelFormat {
        self.data.format()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Volume {
    extent: Extent,
    spacing: Spacing,
    channels: Vec<Channel>,
}

impl Volume {
    pub fn new(extent: Extent, spacing: Spacing, channels: Vec<Channel>) -> Self {
        assert!(!channels.is_empty(), "a volume needs at least one channel");
        assert!(
            channels
                .iter()
                .all(|channel| channel.data().voxel_count() as u64 == extent.voxel_count()),
            "every channel matches the volume extent"
        );
        Self {
            extent,
            spacing,
            channels,
        }
    }

    pub fn extent(&self) -> Extent {
        self.extent
    }

    pub fn spacing(&self) -> Spacing {
        self.spacing
    }

    pub fn channels(&self) -> &[Channel] {
        &self.channels
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    pub fn world_extent(&self) -> [f32; 3] {
        self.spacing.world_extent(self.extent)
    }

    pub fn pyramid(&self) -> Pyramid {
        let level = Level::new(self.extent, self.spacing);
        let channels = self
            .channels
            .iter()
            .map(|channel| {
                let info = match channel.color_table() {
                    Some(color_table) => ChannelInfo::declared(
                        channel.name(),
                        channel.tint(),
                        color_table.indices(),
                        channel.format(),
                    )
                    .with_color_table(color_table.clone())
                    .with_background(channel.background()),
                    None => ChannelInfo::new(
                        channel.name(),
                        channel.tint(),
                        channel.statistics(),
                        channel.format(),
                    ),
                };
                match channel.no_data() {
                    Some(no_data) => info.with_no_data(no_data),
                    None => info,
                }
            })
            .collect();
        Pyramid::new(vec![level], 1, channels, 1, 1.0, Calibration::Recorded)
    }

    pub fn brick(&self, grid: &BrickGrid, coord: BrickCoord) -> BrickData {
        assert_eq!(
            grid.extent(),
            self.extent,
            "the brick grid matches the volume extent"
        );
        let channels = (0..self.channels.len())
            .map(|channel| self.channel_brick(grid, coord, channel))
            .collect();
        BrickData::new(channels)
    }

    pub fn channel_brick(&self, grid: &BrickGrid, coord: BrickCoord, channel: usize) -> VoxelData {
        assert_eq!(
            grid.extent(),
            self.extent,
            "the brick grid matches the volume extent"
        );
        assert!(
            channel < self.channels.len(),
            "the brick channel lies inside the volume"
        );
        pad_brick(
            self.channels[channel].data(),
            [0, 0, 0],
            self.extent,
            grid.origin(coord),
            self.extent,
        )
    }
}
