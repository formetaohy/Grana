use std::time::Duration;

use grana_display::SurfaceSize;
use grana_editor::Frame;
use grana_session::{Session, SessionView};

use super::App;

fn session_view<'a>(
    session: &'a Session,
    export: Option<(u32, u32)>,
    export_message: Option<&str>,
    failure: Option<&'a str>,
) -> SessionView<'a> {
    SessionView {
        access: session.access(),
        revision: session.revision(),
        scene: session.scene(),
        measure: session.measure(),
        measuring: session.measuring(),
        timeline: session.timeline(),
        jobs: session.progress(),
        localizations: session.localizations(),
        io: session.io(export, export_message.map(str::to_owned), failure),
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
    pub(super) fn run_ui(&mut self, size: SurfaceSize) -> (Frame, Option<Duration>) {
        let App {
            surface,
            session,
            engine,
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
        let points = size.points();
        surface.ui.set_language(workspace.language);
        workspace.chrome = chrome.view(&surface.window);
        workspace.chrome.close_only = session.scene().is_none();
        let actions = surface
            .ui
            .update(points, size.scale(), self.start.elapsed().as_secs_f64());
        let failure = engine.as_ref().and_then(grana_engine::Engine::failure);
        let view = session_view(session, export.progress(), output.as_deref(), failure);
        stage.set_surface(size);
        let frame = grana_editor::build(&mut surface.ui, actions, &view, camera, stage, workspace);
        let wait = surface.ui.finish();
        (frame, wait)
    }

    pub(super) fn dispatch(&mut self, message: grana_editor::Message) -> bool {
        let frame = {
            let App {
                surface,
                session,
                engine,
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
            let size = surface.size();
            let failure = engine.as_ref().and_then(grana_engine::Engine::failure);
            let view = session_view(session, export.progress(), output.as_deref(), failure);
            stage.set_surface(size);
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
        let Frame {
            commands,
            window,
            prompt,
            ..
        } = frame;
        if let Some(choice) = prompt {
            self.resolve_prompt(choice);
        }
        let exited = self.apply(commands);
        self.perform_window(window);
        exited || self.exiting
    }
}
