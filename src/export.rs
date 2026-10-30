use crate::scene::Scene;
use grana_object::TrackSet;

pub fn spots(scene: &Scene) -> String {
    let interval = f64::from(scene.dataset.frame_interval());
    let mut csv = String::from("frame,time_s,id,track_id,x_um,y_um,z_um,diameter_um,quality\n");
    for (time, spots) in scene.spot_frames() {
        let seconds = f64::from(time) * interval;
        for (index, spot) in spots.spots().iter().enumerate() {
            let center = spot.center();
            let track = scene
                .spot_track(time, index)
                .map_or(String::new(), |track| (track + 1).to_string());
            csv.push_str(&format!(
                "{time},{seconds:.4},{},{track},{:.4},{:.4},{:.4},{:.4},{:.4}\n",
                index + 1,
                center[0],
                center[1],
                center[2],
                spot.radius() * 2.0,
                spot.quality()
            ));
        }
    }
    csv
}

pub fn surfaces(scene: &Scene) -> String {
    let interval = f64::from(scene.dataset.frame_interval());
    let mut csv = String::from(
        "frame,time_s,id,track_id,voxels,volume_um3,area_um2,sphericity,centroid_x_um,centroid_y_um,centroid_z_um,min_x_um,min_y_um,min_z_um,max_x_um,max_y_um,max_z_um,mean_intensity,min_intensity,max_intensity,intensity_std\n",
    );
    for (time, surfaces) in scene.surface_frames() {
        let seconds = f64::from(time) * interval;
        for (index, object) in surfaces.objects().iter().enumerate() {
            let centroid = object.centroid();
            let bounds = object.bounds();
            let min = bounds.min();
            let max = bounds.max();
            let intensity = object.intensity();
            let track = scene
                .surface_track(time, index)
                .map_or(String::new(), |track| (track + 1).to_string());
            csv.push_str(&format!(
                "{time},{seconds:.4},{},{track},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.2},{:.2},{:.2},{:.2}\n",
                index + 1,
                object.voxel_count(),
                object.volume(),
                object.area(),
                object.sphericity(),
                centroid[0],
                centroid[1],
                centroid[2],
                min[0],
                min[1],
                min[2],
                max[0],
                max[1],
                max[2],
                intensity.mean(),
                intensity.min(),
                intensity.max(),
                intensity.deviation(),
            ));
        }
    }
    csv
}

pub fn tracks(set: &TrackSet) -> String {
    let mut csv = String::from(
        "id,parent,children,start_frame,end_frame,frames,duration_s,length_um,displacement_um,speed_um_s,straightness,start_x_um,start_y_um,start_z_um,end_x_um,end_y_um,end_z_um\n",
    );
    for index in 0..set.len() {
        let track = set.track(index).expect("the track exists");
        let statistics = set.statistics(index).expect("the track has statistics");
        let parent = track
            .parent()
            .map_or(String::new(), |parent| (parent + 1).to_string());
        let children = set
            .children(index)
            .map(|child| (child + 1).to_string())
            .collect::<Vec<_>>()
            .join(";");
        let start = track
            .nodes()
            .first()
            .expect("the track holds nodes")
            .centroid();
        let end = track
            .nodes()
            .last()
            .expect("the track holds nodes")
            .centroid();
        csv.push_str(&format!(
            "{},{parent},{children},{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}\n",
            index + 1,
            track.start(),
            track.end(),
            statistics.frames(),
            statistics.duration(),
            statistics.length(),
            statistics.displacement(),
            statistics.speed(),
            statistics.straightness(),
            start[0],
            start[1],
            start[2],
            end[0],
            end[1],
            end[2],
        ));
    }
    csv
}

pub fn slug(name: &str) -> String {
    let slug = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    if slug.is_empty() {
        "dataset".to_string()
    } else {
        slug
    }
}
