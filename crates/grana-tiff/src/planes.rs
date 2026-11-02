#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    Z,
    C,
    T,
}

const AXES: [Axis; 3] = [Axis::Z, Axis::C, Axis::T];

pub(crate) struct Planes {
    order: Vec<Axis>,
    sizes: [u32; 3],
    chunky: bool,
}

impl Planes {
    pub(crate) fn new(
        size_z: u32,
        size_c: u32,
        size_t: u32,
        chunky: bool,
        dimension_order: &str,
    ) -> Result<Self, String> {
        let sizes = [size_z, size_c, size_t];
        for axis in AXES {
            if sizes[slot(axis)] == 0 {
                return Err("the OME image declares a zero length axis".to_string());
            }
        }
        let mut order = Vec::new();
        let mut seen = [false; 3];
        for character in dimension_order.chars() {
            let Some(axis) = axis(character) else {
                if matches!(character, 'X' | 'Y') {
                    continue;
                }
                return Err(format!(
                    "the OME dimension order `{dimension_order}` names the unknown axis `{character}`"
                ));
            };
            if seen[slot(axis)] {
                return Err(format!(
                    "the OME dimension order `{dimension_order}` names an axis twice"
                ));
            }
            seen[slot(axis)] = true;
            if axis == Axis::C && chunky {
                continue;
            }
            order.push(axis);
        }
        for axis in AXES {
            let inside_page = chunky && axis == Axis::C;
            if !inside_page && sizes[slot(axis)] > 1 && !seen[slot(axis)] {
                return Err(format!(
                    "the OME dimension order `{dimension_order}` omits the {} axis",
                    name(axis)
                ));
            }
        }
        Ok(Self {
            order,
            sizes,
            chunky,
        })
    }

    pub(crate) fn frames(&self) -> u32 {
        self.sizes[slot(Axis::T)]
    }

    pub(crate) fn channels(&self) -> u32 {
        self.sizes[slot(Axis::C)]
    }

    pub(crate) fn count(&self) -> u64 {
        let channels = if self.chunky {
            1
        } else {
            u64::from(self.sizes[slot(Axis::C)])
        };
        u64::from(self.sizes[slot(Axis::Z)]) * u64::from(self.sizes[slot(Axis::T)]) * channels
    }

    pub(crate) fn index(&self, time: u32, channel: u32, z: u32) -> usize {
        let coordinates = [z, channel, time];
        let mut stride = 1u64;
        let mut index = 0u64;
        for axis in &self.order {
            let size = self.sizes[slot(*axis)];
            assert!(
                coordinates[slot(*axis)] < size,
                "the plane coordinate lies inside the image"
            );
            index += u64::from(coordinates[slot(*axis)]) * stride;
            stride *= u64::from(size);
        }
        index as usize
    }
}

fn axis(character: char) -> Option<Axis> {
    match character {
        'Z' => Some(Axis::Z),
        'C' => Some(Axis::C),
        'T' => Some(Axis::T),
        _ => None,
    }
}

fn name(axis: Axis) -> &'static str {
    match axis {
        Axis::Z => "Z",
        Axis::C => "C",
        Axis::T => "T",
    }
}

fn slot(axis: Axis) -> usize {
    match axis {
        Axis::Z => 0,
        Axis::C => 1,
        Axis::T => 2,
    }
}
