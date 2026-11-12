use std::time::Instant;

use grana_animation::Keyframe;
use grana_engine::Display;

use super::{App, PLAYBACK_INTERVAL};

impl App {
    pub(super) fn display_mode(&self, animating: bool) -> Display {
        if animating || self.interacting || self.workspace.playback.previewing {
            Display::Interactive
        } else if self.workspace.playback.playing {
            Display::Playback
        } else {
            Display::Still
        }
    }

    pub(super) fn set_keyframe(&mut self) {
        let Some(scene) = self.session.scene() else {
            return;
        };
        let duration = self.session.timeline().duration();
        let at = self.workspace.playback.position.clamp(0.0, duration);
        let key = Keyframe {
            at,
            time: scene.time(),
            camera: self.camera.state(),
        };
        let index = self.session.insert_keyframe(key);
        self.workspace.playback.position = at;
        self.workspace.playback.selected = Some(index);
        self.output = None;
        self.request_redraw();
    }

    pub(super) fn advance_playback(&mut self) {
        if !self.workspace.playback.playing {
            return;
        }
        let Some(scene) = self.session.scene() else {
            self.workspace.playback.playing = false;
            return;
        };
        let frames = scene.frame_count();
        if frames <= 1 {
            self.workspace.playback.playing = false;
            return;
        }
        let current = scene.time();
        if self.frame_info().samples == 0 {
            return;
        }
        let now = Instant::now();
        let due = self
            .last_advance
            .is_none_or(|last| now.duration_since(last) >= PLAYBACK_INTERVAL);
        if !due {
            return;
        }
        let next = (current + 1) % frames;
        self.session.set_time(next);
        self.workspace.playback.previewing = false;
        self.last_advance = Some(now);
    }

    pub(super) fn advance_animation(&mut self) {
        if !self.workspace.playback.previewing {
            return;
        }
        let duration = self.session.timeline().duration();
        let now = Instant::now();
        if let Some(last) = self.last_animation.replace(now) {
            let elapsed = now.duration_since(last).as_secs_f32().min(0.25);
            self.workspace.playback.position =
                (self.workspace.playback.position + elapsed) % duration;
        }
        self.pose_animation();
    }

    pub(super) fn pose_animation(&mut self) {
        let position = self.workspace.playback.position;
        if let Some(sample) = self.session.timeline().sample(position) {
            self.camera.restore(sample.camera);
            self.session.set_time(sample.time);
        }
    }

    pub(super) fn toggle_play(&mut self) {
        let Some(scene) = self.session.scene() else {
            return;
        };
        if scene.frame_count() <= 1 {
            return;
        }
        let playing = !self.workspace.playback.playing;
        self.workspace.playback.playing = playing;
        self.last_advance = playing.then(Instant::now);
        if playing {
            self.workspace.playback.previewing = false;
            self.last_animation = None;
        }
        self.request_redraw();
    }

    pub(super) fn step_slice(&mut self, steps: i32) {
        let Some(scene) = self.session.scene() else {
            return;
        };
        grana_editor::slice_step(scene, &mut self.camera, steps);
    }

    pub(super) fn step_time(&mut self, delta: i64) {
        let Some(scene) = self.session.scene() else {
            return;
        };
        let frames = scene.frame_count();
        if frames <= 1 {
            return;
        }
        let time = (i64::from(scene.time()) + delta).rem_euclid(i64::from(frames)) as u32;
        self.workspace.playback.playing = false;
        self.workspace.playback.previewing = false;
        self.last_advance = None;
        self.last_animation = None;
        self.session.set_time(time);
    }
}
