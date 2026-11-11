use grana_session::{Command, SessionView};
use grana_ui::Vec2 as UiVec2;

use super::App;
impl App {
    pub(super) fn run_ui(&mut self) -> (Vec<Command>, bool) {
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
            return (Vec::new(), false);
        };
        let scale = surface.window.scale_factor() as f32;
        let size = surface.gpu.size();
        let points = UiVec2::new(size[0] as f32 / scale, size[1] as f32 / scale);
        surface.ui.resize(size);
        surface.ui.set_language(workspace.language);
        surface.ui.set_accessibility(grana_ui::Accessibility {
            reduce_transparency: workspace.reduce_transparency,
            increase_contrast: workspace.contrast,
        });
        let actions = surface
            .ui
            .update(points, scale, self.start.elapsed().as_secs_f64());
        let exporting = export.progress();
        let export_message = output.clone();
        let view = SessionView {
            scene: session.scene(),
            measure: session.measure(),
            measuring: session.measuring(),
            timeline: session.timeline(),
            jobs: session.progress(),
            io: session.io(exporting, export_message),
            batch: session.batch(),
            history: session.history(),
            series: session.series(),
            plate: session.plate(),
            labels: session.label_images(),
            views: session.views(),
            recent: session.recent(),
            import: session.import(),
            recovery: session.recovery(),
            export: session.export(),
        };
        let commands =
            grana_editor::build(&mut surface.ui, actions, &view, camera, stage, workspace);
        let animating = surface.ui.finish();
        (commands, animating)
    }

    pub(super) fn dispatch(&mut self, message: grana_editor::Message) -> bool {
        let commands = {
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
            let view = SessionView {
                scene: session.scene(),
                measure: session.measure(),
                measuring: session.measuring(),
                timeline: session.timeline(),
                jobs: session.progress(),
                io: session.io(export.progress(), output.clone()),
                batch: session.batch(),
                history: session.history(),
                series: session.series(),
                plate: session.plate(),
                labels: session.label_images(),
                views: session.views(),
                recent: session.recent(),
                import: session.import(),
                recovery: session.recovery(),
                export: session.export(),
            };
            let mut commands = Vec::new();
            grana_editor::apply(
                &mut surface.ui,
                vec![message],
                &view,
                camera,
                stage,
                workspace,
                &mut commands,
            );
            commands
        };
        self.apply(commands)
    }
}
