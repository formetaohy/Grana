use std::time::Duration;

use grana_editor::Frame;
use grana_session::{Session, SessionView};
use grana_ui::Vec2 as UiVec2;

use super::App;

fn session_view<'a>(
    session: &'a Session,
    export: Option<(u32, u32)>,
    message: Option<&str>,
) -> SessionView<'a> {
    SessionView {
        access: session.access(),
        scene: session.scene(),
        measure: session.measure(),
        measuring: session.measuring(),
        timeline: session.timeline(),
        jobs: session.progress(),
        localizations: session.localizations(),
        io: session.io(export, message.map(str::to_owned)),
        batch: session.batch(),
        stitch: session.stitch().map(|stitch| stitch.view()),
        history: session.history(),
        series: session.series(),
        plate: session.plate(),
        labels: session.label_images(),
        views: session.views(),
        recent: session.recent(),
        import: session.import(),
        recovery: session.recovery(),
        export: session.export(),
    }
}

impl App {
    pub(super) fn run_ui(&mut self) -> (Frame, Option<Duration>) {
        let App {
            surface,
            session,
            camera,
            stage,
            workspace,
            export,
            output,
            chrome,
            ..
        } = self;
        let Some(surface) = surface.as_mut() else {
            return (Frame::default(), None);
        };
        let scale = surface.window.scale_factor() as f32;
        let size = surface.gpu.size();
        let points = UiVec2::new(size[0] as f32 / scale, size[1] as f32 / scale);
        surface.ui.resize(size);
        surface.ui.set_language(workspace.language);
        workspace.chrome = chrome.view(&surface.window);
        workspace.chrome.close_only = session.scene().is_none();
        let actions = surface
            .ui
            .update(points, scale, self.start.elapsed().as_secs_f64());
        let view = session_view(session, export.progress(), output.as_deref());
        let frame = grana_editor::build(&mut surface.ui, actions, &view, camera, stage, workspace);
        let wait = surface.ui.finish();
        (frame, wait)
    }

    pub(super) fn dispatch(&mut self, message: grana_editor::Message) -> bool {
        let frame = {
            let App {
                surface,
                session,
                camera,
                stage,
                workspace,
                export,
                output,
                ..
            } = self;
            let Some(surface) = surface.as_mut() else {
                return false;
            };
            let view = session_view(session, export.progress(), output.as_deref());
            let mut frame = Frame::default();
            grana_editor::apply(
                &mut surface.ui,
                vec![message],
                &view,
                camera,
                stage,
                workspace,
                &mut frame,
            );
            frame
        };
        let exited = self.apply(frame.commands);
        self.perform_window(frame.window);
        exited
    }
}
