use std::collections::HashSet;

use grana_segment::{Flow, Settings, assemble};

fn radial_flow(side: usize, centers: &[(f32, f32)], radius: f32) -> Flow {
    let area = side * side;
    let mut dy = vec![0f32; area];
    let mut dx = vec![0f32; area];
    let mut cellprob = vec![-1f32; area];
    for y in 0..side {
        for x in 0..side {
            for &(center_y, center_x) in centers {
                let offset_y = center_y - y as f32;
                let offset_x = center_x - x as f32;
                if offset_y * offset_y + offset_x * offset_x <= radius * radius {
                    let magnitude = (offset_y * offset_y + offset_x * offset_x).sqrt();
                    if magnitude > 0.0 {
                        dy[y * side + x] = offset_y / magnitude;
                        dx[y * side + x] = offset_x / magnitude;
                    }
                    cellprob[y * side + x] = 1.0;
                    break;
                }
            }
        }
    }
    Flow::from_parts(side as u32, side as u32, dy, dx, cellprob)
}

fn disk_plane(width: usize, height: usize, disks: &[(u32, u32, u32, u32)]) -> Vec<u32> {
    let mut labels = vec![0u32; width * height];
    for &(center_x, center_y, radius, label) in disks {
        for y in 0..height {
            for x in 0..width {
                let offset_x = x as i64 - center_x as i64;
                let offset_y = y as i64 - center_y as i64;
                if offset_x * offset_x + offset_y * offset_y
                    <= i64::from(radius) * i64::from(radius)
                {
                    labels[y * width + x] = label;
                }
            }
        }
    }
    labels
}

#[test]
fn radial_flow_masks_two_disks() {
    let side = 96;
    let centers = [(28.0f32, 28.0f32), (68.0, 60.0)];
    let radius = 12.0;
    let flow = radial_flow(side, &centers, radius);
    assert_eq!(flow.width(), side as u32);
    assert_eq!(flow.height(), side as u32);
    assert_eq!(flow.dy().len(), side * side);
    assert_eq!(flow.dx().len(), side * side);
    assert_eq!(flow.cellprob().len(), side * side);

    let masks = flow.masks(0.0);
    assert_eq!(masks.len(), side * side);
    let labels: HashSet<u32> = masks.iter().copied().filter(|&label| label != 0).collect();
    assert_eq!(labels.len(), 2);
    for y in 0..side {
        for x in 0..side {
            let mut inside = false;
            for &(center_y, center_x) in &centers {
                let offset_y = center_y - y as f32;
                let offset_x = center_x - x as f32;
                if offset_y * offset_y + offset_x * offset_x <= radius * radius {
                    inside = true;
                    break;
                }
            }
            if inside {
                assert_ne!(
                    masks[y * side + x],
                    0,
                    "disk pixel ({x}, {y}) is unlabelled"
                );
            } else {
                assert_eq!(
                    masks[y * side + x],
                    0,
                    "background pixel ({x}, {y}) is labelled"
                );
            }
        }
    }
}

#[test]
fn assemble_links_and_separates_disks() {
    let width = 32;
    let height = 32;
    let settings = Settings::new(0.0, 0.4, 15, 32, 0.5);
    assert_eq!(settings.cellprob_threshold(), 0.0);
    assert_eq!(settings.flow_threshold(), 0.4);
    assert_eq!(settings.minimum_size(), 15);
    assert_eq!(settings.tile_overlap(), 32);
    assert_eq!(settings.slice_overlap(), 0.5);

    let first = disk_plane(width, height, &[(10, 16, 4, 1)]);
    let second = disk_plane(width, height, &[(12, 16, 4, 1)]);
    let linked = assemble(
        &[first, second],
        width as u32,
        height as u32,
        settings.slice_overlap(),
    )
    .expect("shifted slices must link");
    assert_eq!(linked.width(), width as u32);
    assert_eq!(linked.height(), height as u32);
    assert_eq!(linked.depth(), 2);
    assert_eq!(linked.labels().len(), width * height * 2);
    assert_eq!(linked.count(), 1);
    assert!(linked.labels().contains(&1));
    assert!(linked.labels().iter().all(|&label| label <= 1));

    let first = disk_plane(width, height, &[(10, 10, 4, 1), (24, 24, 4, 2)]);
    let second = disk_plane(width, height, &[(12, 10, 4, 1), (26, 24, 4, 2)]);
    let separate = assemble(
        &[first, second],
        width as u32,
        height as u32,
        settings.slice_overlap(),
    )
    .expect("shifted disks must link per object");
    assert_eq!(separate.count(), 2);
    let labels = separate.labels();
    let left_first = labels[10 * width + 10];
    let left_second = labels[width * height + 10 * width + 12];
    let right_first = labels[24 * width + 24];
    let right_second = labels[width * height + 24 * width + 26];
    assert_eq!(left_first, left_second);
    assert_eq!(right_first, right_second);
    assert_ne!(left_first, right_first);
    assert_ne!(left_first, 0);
    assert_ne!(right_first, 0);
}
