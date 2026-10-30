mod app;
mod dataset;
mod egui_layer;
mod gpu;
mod scene;
mod transfer_editor;
mod ui;
mod viewport;

fn main() {
    let event_loop = winit::event_loop::EventLoop::new().expect("failed to create the event loop");
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
    let mut app = app::App::new();
    event_loop
        .run_app(&mut app)
        .expect("the event loop terminated abnormally");
}
